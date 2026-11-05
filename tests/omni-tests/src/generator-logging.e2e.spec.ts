/**
 * Generator script logging - a `run-javascript` action's `ctx.log.*` calls are
 * forwarded over the bridge `/log` RPC into omni's global logger and rendered
 * (`info`/`warn`/`error` always, `debug`/`trace` under `--trace`). Pinned to
 * `crates/bridge_rpc_services/src/services/log.rs` (host-side forwarding into
 * `log::logger()` at the mapped level, target = joined category) and the
 * bridge-service log pipeline (`packages/bridge-rpc-services/src/exec-script/service.ts`,
 * `services/bridge-service/src/entrypoint/cli.ts`).
 *
 * Script logs are rendered by the CLI presenter on stderr; stdout is results only.
 *
 * These tests require a JS runtime (node/bun/deno) on PATH; the runner
 * auto-detects one.
 */

import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    runOmni,
    type WorkspaceSpec,
} from "@/harness";

/**
 * A workspace whose `logger` generator runs {@link script} via a single
 * `run-javascript` action. The generator declares no prompts, so runs are
 * non-interactive under `--use-defaults`.
 */
function loggingGeneratorSpec(script: string): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/logger/generator.omni.yaml": {
                name: "logger",
                description: "runs a JS generator script that logs",
                actions: [{ type: "run-javascript", script: "gen.mjs" }],
            },
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "generators/logger/gen.mjs": script,
        },
    };
}

/** Non-interactive `generator run` args for the `logger` generator above. */
const RUN_ARGS = [
    "generator",
    "run",
    "-n",
    "logger",
    "-o",
    "out",
    "--use-defaults",
    "--save-session=false",
];

/** An `export default` script whose body is `body`. */
function script(body: string): string {
    return `export default async function (ctx) {\n${body}\n}\n`;
}

describe(
    "+generator @e2e @output (script logging)",
    {
        tags: ["generator"],
    },
    () => {
        it("surfaces ctx.log.info output on stderr by default", async () => {
            const ws = makeWorkspace(
                loggingGeneratorSpec(
                    script(`ctx.log.info("GEN_LOG_INFO_MARKER");`),
                ),
            );

            const result = await runOmni(RUN_ARGS, {
                cwd: ws.cwd,
            });

            expect(result).toHaveSucceeded();
            expect(result).toHaveStderrContaining("GEN_LOG_INFO_MARKER");
        });

        it.each([false, true])(
            "redacts secret log messages and targets (failure=%s)",
            async (fail) => {
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [
                            { source: "local", path: "generators/**" },
                        ],
                    },
                    projects: {
                        "generators/logger/generator.omni.yaml": {
                            name: "logger",
                            description: "logs a secret input",
                            inputs: [
                                {
                                    type: "string",
                                    name: "pw",
                                    message: "pw",
                                    secret: true,
                                },
                            ],
                            actions: [
                                {
                                    type: "run-javascript",
                                    script: "gen.mjs",
                                    data: { pw: "{{ inputs.pw }}" },
                                },
                            ],
                        },
                    },
                    files: {
                        ".omni/sources/generator/.keep": "",
                        "generators/logger/gen.mjs": script(
                            `ctx.log.child(ctx.data.pw).info("pw is " + ctx.data.pw);
                         ${fail ? 'throw new Error("intentional failure");' : ""}`,
                        ),
                    },
                });

                const result = await runOmni(
                    [
                        "--trace",
                        "debug",
                        "--trace-file",
                        ws.path("trace.jsonl"),
                        "--trace-file-level",
                        "debug",
                        ...RUN_ARGS,
                        "-v",
                        "pw=hunter2-secret",
                    ],
                    { cwd: ws.cwd },
                );

                if (fail) {
                    expect(result).toHaveFailed();
                    expect(result).toHaveStderrContaining(
                        "script log discarded after failure",
                    );
                } else {
                    expect(result).toHaveSucceeded();
                    expect(result).toHaveStderrContaining("pw is ");
                    expect(result).toHaveStderrContaining("script_target");
                }
                expect(result.err).not.toContain("hunter2-secret");
                expect(result.out).not.toContain("hunter2-secret");
                expect(ws.read("trace.jsonl")).not.toContain("hunter2-secret");
            },
        );

        it("redacts messages and targets before the default MCP tracing callback", async () => {
            const ws = makeWorkspace({
                ...loggingGeneratorSpec(
                    script(
                        `ctx.log.child(ctx.data.pw).info("MCP_LOG " + ctx.data.pw);`,
                    ),
                ),
                projects: {
                    "generators/logger/generator.omni.yaml": {
                        name: "logger",
                        description: "logs a secret input through MCP",
                        inputs: [
                            {
                                type: "string",
                                name: "pw",
                                message: "pw",
                                secret: true,
                            },
                        ],
                        actions: [
                            {
                                type: "run-javascript",
                                script: "gen.mjs",
                                data: { pw: "{{ inputs.pw }}" },
                            },
                        ],
                    },
                },
            });
            const mcp = await connectMcp({
                cwd: ws.cwd,
                args: [
                    "--trace-file",
                    ws.path("trace.jsonl"),
                    "--trace-file-level",
                    "debug",
                ],
            });
            const result = await mcp.client.callTool({
                name: "generator_run",
                arguments: {
                    name: "logger",
                    output_dir: ws.path("out"),
                    input_values: { pw: "mcp-secret-token" },
                    use_defaults: true,
                    save_session: false,
                    ignore_session: true,
                },
            });
            expect(result.isError).not.toBe(true);
            await mcp.disconnect();
            const trace = ws.read("trace.jsonl");
            expect(trace).toContain("MCP_LOG");
            expect(trace).toContain("script_target");
            expect(trace).not.toContain("mcp-secret-token");
        });

        it("maps log levels: info/warn/error show by default, debug/trace only under --trace", async () => {
            const ws = makeWorkspace(
                loggingGeneratorSpec(
                    script(
                        [
                            `ctx.log.warn("GEN_LOG_WARN");`,
                            `ctx.log.error("GEN_LOG_ERROR");`,
                            `ctx.log.debug("GEN_LOG_DEBUG");`,
                            `ctx.log.trace("GEN_LOG_TRACE");`,
                        ].join("\n"),
                    ),
                ),
            );

            // The script only logs (no filesystem writes), so the same workspace
            // can be reused across runs at different levels without conflicts.
            const atInfo = await runOmni([...RUN_ARGS], {
                cwd: ws.cwd,
            });
            expect(atInfo).toHaveSucceeded();
            expect(atInfo).toHaveStderrContaining("GEN_LOG_WARN");
            expect(atInfo).toHaveStderrContaining("GEN_LOG_ERROR");
            expect(atInfo.out).not.toContain("GEN_LOG_DEBUG");
            expect(atInfo.out).not.toContain("GEN_LOG_TRACE");

            const atTrace = await runOmni(["--trace", "trace", ...RUN_ARGS], {
                cwd: ws.cwd,
            });
            expect(atTrace).toHaveSucceeded();
            expect(atTrace).toHaveStderrContaining("GEN_LOG_DEBUG");
            expect(atTrace).toHaveStderrContaining("GEN_LOG_TRACE");
        });

        it(
            "delivers a log emitted at the very end of a script (flushed before the response)",
            {
                // A few iterations: the bug this guards against was intermittent (the
                // tail log racing connection teardown), so repeat to make a regression
                // unlikely to slip through as a lucky pass.
                timeout: 60_000,
            },
            async () => {
                // The log is the final statement, immediately before the script
                // returns. Without the bridge-service flush-before-response, this tail
                // line is intermittently dropped.
                const ws = makeWorkspace(
                    loggingGeneratorSpec(
                        script(
                            [
                                `await ctx.sys.fs.writeStringToFile("marker.txt", "x");`,
                                `ctx.log.info("GEN_LOG_TAIL_MARKER");`,
                            ].join("\n"),
                        ),
                    ),
                );

                for (let i = 0; i < 3; i++) {
                    const result = await runOmni([...RUN_ARGS], {
                        cwd: ws.cwd,
                    });
                    expect(result).toHaveSucceeded();
                    expect(result).toHaveStderrContaining(
                        "GEN_LOG_TAIL_MARKER",
                    );
                }
            },
        );

        it("delivers logs from a nested run-generator child script", async () => {
            const ws = makeWorkspace({
                workspace: {
                    projects: ["**"],
                    generators: [{ source: "local", path: "generators/**" }],
                },
                projects: {
                    "generators/child/generator.omni.yaml": {
                        name: "child",
                        description: "child generator that logs",
                        actions: [
                            { type: "run-javascript", script: "gen.mjs" },
                        ],
                    },
                    "generators/parent/generator.omni.yaml": {
                        name: "parent",
                        description: "parent generator that logs and delegates",
                        actions: [
                            { type: "run-javascript", script: "gen.mjs" },
                            { type: "run-generator", generator: "child" },
                        ],
                    },
                },
                files: {
                    ".omni/sources/generator/.keep": "",
                    "generators/parent/gen.mjs": script(
                        `ctx.log.info("GEN_LOG_PARENT");`,
                    ),
                    "generators/child/gen.mjs": script(
                        `ctx.log.info("GEN_LOG_CHILD");`,
                    ),
                },
            });

            const result = await runOmni(
                [
                    "generator",
                    "run",
                    "-n",
                    "parent",
                    "-o",
                    "out",
                    "--use-defaults",
                    "--save-session=false",
                ],
                { cwd: ws.cwd },
            );

            expect(result).toHaveSucceeded();
            expect(result).toHaveStderrContaining("GEN_LOG_PARENT");
            expect(result).toHaveStderrContaining("GEN_LOG_CHILD");
        });

        it("surfaces logs under --dry-run while committing nothing", async () => {
            const ws = makeWorkspace(
                loggingGeneratorSpec(
                    script(
                        [
                            `await ctx.sys.fs.writeStringToFile("from-js.txt", "x");`,
                            `ctx.log.info("GEN_LOG_DRYRUN");`,
                        ].join("\n"),
                    ),
                ),
            );

            const result = await runOmni([...RUN_ARGS, "--dry-run"], {
                cwd: ws.cwd,
            });

            expect(result).toHaveSucceeded();
            // The script ran (and logged), but its write was rolled back.
            expect(result).toHaveStderrContaining("GEN_LOG_DRYRUN");
            expect(ws.exists("out/from-js.txt")).toBe(false);
            expect(ws.exists("from-js.txt")).toBe(false);
        });
    },
);
