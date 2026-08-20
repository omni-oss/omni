/**
 * Generator script logging - a `run-javascript` action's `ctx.log.*` calls are
 * forwarded over the bridge `/log` RPC into omni's global logger and rendered
 * per the CLI log-level config. Pinned to
 * `crates/bridge_rpc_services/src/services/log.rs` (host-side forwarding into
 * `log::logger()` at the mapped level, target = joined category) and the
 * bridge-service log pipeline (`packages/bridge-rpc-services/src/exec-script/service.ts`,
 * `services/bridge-service/src/entrypoint/cli.ts`).
 *
 * Default CLI levels (crates/omni_cli_core/src/commands/mod.rs): stdout log
 * level defaults to `info`, stderr to `off` - so generator logs land on stdout.
 *
 * These tests require a JS runtime (node/bun/deno) on PATH; the runner
 * auto-detects one.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

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

describe("+generator @e2e @output (script logging)", {
    tags: ["generator"],
}, () => {
    it("surfaces ctx.log.info output on stdout at the default log level", async () => {
        const ws = makeWorkspace(
            loggingGeneratorSpec(
                script(`ctx.log.info("GEN_LOG_INFO_MARKER");`),
            ),
        );

        // Clear any inherited OMNI_STDOUT_LOG_LEVEL so the CLI's own default
        // (`info`) governs - this asserts the out-of-the-box behavior.
        const result = await runOmni(RUN_ARGS, {
            cwd: ws.cwd,
            env: { OMNI_STDOUT_LOG_LEVEL: undefined },
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("GEN_LOG_INFO_MARKER");
    });

    it("maps log levels: warn/error show at info, debug/trace are gated by -l", async () => {
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
        const atInfo = await runOmni(["-l", "info", ...RUN_ARGS], {
            cwd: ws.cwd,
        });
        expect(atInfo).toHaveSucceeded();
        expect(atInfo).toOutputContaining("GEN_LOG_WARN");
        expect(atInfo).toOutputContaining("GEN_LOG_ERROR");
        expect(atInfo.out).not.toContain("GEN_LOG_DEBUG");
        expect(atInfo.out).not.toContain("GEN_LOG_TRACE");

        const atTrace = await runOmni(["-l", "trace", ...RUN_ARGS], {
            cwd: ws.cwd,
        });
        expect(atTrace).toHaveSucceeded();
        expect(atTrace).toOutputContaining("GEN_LOG_DEBUG");
        expect(atTrace).toOutputContaining("GEN_LOG_TRACE");
    });

    it("delivers a log emitted at the very end of a script (flushed before the response)", {
        // A few iterations: the bug this guards against was intermittent (the
        // tail log racing connection teardown), so repeat to make a regression
        // unlikely to slip through as a lucky pass.
        timeout: 60_000,
    }, async () => {
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
            const result = await runOmni(["-l", "info", ...RUN_ARGS], {
                cwd: ws.cwd,
                env: { OMNI_STDOUT_LOG_LEVEL: undefined },
            });
            expect(result).toHaveSucceeded();
            expect(result).toOutputContaining("GEN_LOG_TAIL_MARKER");
        }
    });

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
                    actions: [{ type: "run-javascript", script: "gen.mjs" }],
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
                "-l",
                "info",
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd, env: { OMNI_STDOUT_LOG_LEVEL: undefined } },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("GEN_LOG_PARENT");
        expect(result).toOutputContaining("GEN_LOG_CHILD");
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

        const result = await runOmni(["-l", "info", ...RUN_ARGS, "--dry-run"], {
            cwd: ws.cwd,
            env: { OMNI_STDOUT_LOG_LEVEL: undefined },
        });

        expect(result).toHaveSucceeded();
        // The script ran (and logged), but its write was rolled back.
        expect(result).toOutputContaining("GEN_LOG_DRYRUN");
        expect(ws.exists("out/from-js.txt")).toBe(false);
        expect(ws.exists("from-js.txt")).toBe(false);
    });
});
