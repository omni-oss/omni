/** RFC 0018 acceptance tests, exercised through the real stdio MCP transport. */
import { spawnSync } from "node:child_process";
import { readdirSync, symlinkSync, unlinkSync } from "node:fs";

import type { Client } from "@modelcontextprotocol/client";
import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    scaffoldGeneratorSpec,
    singleProjectSpec,
} from "@/harness";
import { resolveOmniBin } from "@/harness/binary";

import { cleanPath } from "./utils";

function launchDir() {
    const dir = makeWorkspace();
    unlinkSync(dir.path("workspace.omni.yaml"));
    return dir;
}

async function content(
    client: Client,
    name: string,
    working_dir: string,
    args = {},
) {
    const result = await client.callTool({
        name,
        arguments: { working_dir, ...args },
    });
    expect(result.isError).not.toBe(true);
    expect(result.structuredContent).toBeDefined();
    return result.structuredContent as Record<string, any>;
}

describe(
    "+mcp @mcp @cli (shared acceptance)",
    { tags: ["mcp"], timeout: 60_000 },
    () => {
        it("initializes and lists tools outside a workspace without creating launch-directory files", async () => {
            const launch = launchDir();
            const { client } = await connectMcp({
                cwd: launch.cwd,
                shared: true,
            });
            expect((await client.listTools()).tools).toHaveLength(22);
            expect(readdirSync(launch.cwd)).toEqual([]);
        });

        it("rejects explicit and environment-supplied root-dir conflicts in child processes", () => {
            const launch = launchDir();
            for (const fromEnv of [false, true]) {
                const result = spawnSync(
                    resolveOmniBin(),
                    [
                        "mcp",
                        "--shared",
                        ...(fromEnv ? [] : ["--root-dir", launch.cwd]),
                    ],
                    {
                        cwd: launch.cwd,
                        env: {
                            ...process.env,
                            OMNI_ROOT_DIR: fromEnv ? launch.cwd : "",
                        },
                        encoding: "utf8",
                        timeout: 10_000,
                    },
                );
                expect(result.status).toBe(2);
                expect(result.stderr).toMatch(/cannot be used with/);
                expect(result.stderr).toContain("--root-dir");
            }
        });

        it("preserves bound schemas, operation fields, defaults and annotations while requiring one shared directory", async () => {
            const ws = makeWorkspace(singleProjectSpec());
            const bound = await connectMcp({ cwd: ws.cwd });
            const shared = await connectMcp({
                cwd: launchDir().cwd,
                shared: true,
            });
            const baseline = (await bound.client.listTools()).tools;
            const tools = (await shared.client.listTools()).tools;
            for (const original of baseline) {
                const tool = tools.find((t) => t.name === original.name)!;
                expect(tool.annotations).toEqual(original.annotations);
                expect(
                    tool.inputSchema.required?.filter(
                        (k) => k === "working_dir",
                    ),
                ).toEqual(["working_dir"]);
                expect(tool.inputSchema.properties?.working_dir).toMatchObject({
                    type: "string",
                });
                for (const [key, value] of Object.entries(
                    original.inputSchema.properties ?? {},
                )) {
                    if (key !== "working_dir")
                        expect(tool.inputSchema.properties?.[key]).toEqual(
                            value,
                        );
                }
                for (const key of original.inputSchema.required ?? [])
                    expect(tool.inputSchema.required).toContain(key);
            }
            expect(
                baseline.find((t) => t.name === "workspace_info")!.inputSchema
                    .properties,
            ).toEqual({});
            expect(
                baseline.find((t) => t.name === "tool_run")!.inputSchema
                    .required,
            ).not.toContain("working_dir");
            expect(
                cleanPath(
                    (await content(bound.client, "workspace_info", ws.cwd))
                        .root_dir,
                ),
            ).toBe(ws.cwd);
            for (const name of [
                "workspace_info",
                "generator_list",
                "hash_workspace",
                "tool_list",
            ]) {
                await content(shared.client, name, ws.cwd);
            }
        });

        it("rejects invalid directories locally and recovers after every error", async () => {
            const ws = makeWorkspace(singleProjectSpec());
            const launch = launchDir();
            const { client } = await connectMcp({
                cwd: launch.cwd,
                shared: true,
            });
            for (const working_dir of [
                undefined,
                null,
                "",
                "app",
                "~/app",
                ws.path("missing"),
                ws.path("workspace.omni.yaml"),
            ]) {
                await expect(
                    client.callTool({
                        name: "workspace_info",
                        arguments:
                            working_dir === undefined ? {} : { working_dir },
                    }),
                ).rejects.toThrow(/working_dir/);
                expect(
                    cleanPath(
                        (await content(client, "workspace_info", ws.cwd))
                            .root_dir,
                    ),
                ).toBe(ws.cwd);
            }
            await expect(
                content(client, "workspace_info", launch.cwd),
            ).rejects.toThrow(/workspace/);
            await expect(
                client.callTool({ name: "not_a_tool", arguments: {} }),
            ).rejects.toThrow(/unknown tool/);
            // Validate operation shape before trying to discover a workspace.
            await expect(
                content(client, "generator_inspect", launch.cwd),
            ).rejects.toThrow(/name/);
            await content(client, "workspace_info", ws.cwd);
        });

        for (const ext of ["yaml", "yml", "json", "toml"]) {
            it(`discovers the nearest nested workspace with a ${ext} marker`, async () => {
                const ws = makeWorkspace({ files: { "inner/deep/.keep": "" } });
                ws.write(`inner/workspace.omni.${ext}`, { projects: [] });
                const { client } = await connectMcp({
                    cwd: launchDir().cwd,
                    shared: true,
                });
                expect(
                    cleanPath(
                        (
                            await content(
                                client,
                                "workspace_info",
                                ws.path("inner/deep"),
                            )
                        ).root_dir,
                    ),
                ).toBe(ws.path("inner"));
                expect(
                    cleanPath(
                        (await content(client, "workspace_info", ws.cwd))
                            .root_dir,
                    ),
                ).toBe(ws.cwd);
            });
        }

        it.skipIf(process.platform === "win32")(
            "discovers physical symlink ancestors rather than alias ancestors",
            async () => {
                const ws = makeWorkspace({ files: { "physical/.keep": "" } });
                const alias = makeWorkspace();
                symlinkSync(ws.path("physical"), alias.path("link"), "dir");
                const { client } = await connectMcp({
                    cwd: launchDir().cwd,
                    shared: true,
                });
                expect(
                    cleanPath(
                        (
                            await content(
                                client,
                                "workspace_info",
                                alias.path("link"),
                            )
                        ).root_dir,
                    ),
                ).toBe(ws.cwd);
            },
        );

        it("isolates invocation environments and relative cache overrides across alternating and concurrent calls", async () => {
            const a = makeWorkspace({
                files: {
                    ".env": "SHARED_TEST=root-a\n",
                    "sub/.env": "SHARED_TEST=sub-a\n",
                },
            });
            const b = makeWorkspace({
                files: { ".env": "SHARED_TEST=root-b\n" },
            });
            const launch = launchDir();
            const { client } = await connectMcp({
                cwd: launch.cwd,
                shared: true,
                args: ["--cache-dir", "local-cache"],
            });
            const check = async (dir: string, marker: string) => {
                const data = await content(client, "workspace_info", dir);
                expect(data.env_vars.SHARED_TEST).toBe(marker);
                expect(cleanPath(data.cache_dir)).toBe(
                    `${cleanPath(dir)}/local-cache`,
                );
            };
            await check(a.cwd, "root-a");
            await check(b.cwd, "root-b");
            await check(a.path("sub"), "sub-a");
            await Promise.all(
                Array.from({ length: 8 }, (_, i) =>
                    i % 2
                        ? check(a.path("sub"), "sub-a")
                        : check(b.cwd, "root-b"),
                ),
            );
            expect(process.env.SHARED_TEST).toBeUndefined();
            expect(readdirSync(launch.cwd)).toEqual([]);
        });

        it("observes workspace/project edits and recovers from invalid configuration", async () => {
            const ws = makeWorkspace(singleProjectSpec());
            const { client } = await connectMcp({
                cwd: launchDir().cwd,
                shared: true,
            });
            expect(
                (await content(client, "project_list", ws.cwd)).projects,
            ).toEqual(["app"]);
            ws.write("app/project.omni.yaml", {
                name: "renamed",
                tasks: { build: "echo changed" },
            });
            expect(
                (await content(client, "project_list", ws.cwd)).projects,
            ).toEqual(["renamed"]);
            ws.write("workspace.omni.yaml", "projects: [broken\n");
            await expect(
                content(client, "workspace_info", ws.cwd),
            ).rejects.toThrow(/workspace configuration/);
            ws.write("workspace.omni.yaml", { projects: [] });
            expect(
                (await content(client, "project_list", ws.cwd)).projects,
            ).toEqual([]);
        });

        it("resolves generator output from the invocation directory", async () => {
            const ws = makeWorkspace(scaffoldGeneratorSpec());
            ws.write("sub/.keep", "");
            const { client } = await connectMcp({
                cwd: launchDir().cwd,
                shared: true,
            });
            await content(client, "generator_run", ws.path("sub"), {
                name: "scaffold",
                output_dir: "out",
                dry_run: false,
                use_defaults: true,
                save_session: false,
                ignore_session: true,
            });
            expect(ws.read("sub/out/src/greeting.txt")).toContain(
                "Hello world!",
            );
            expect(ws.exists("out")).toBe(false);
        });

        it("serializes generators sharing a workspace across fresh request contexts", async () => {
            const ws = makeWorkspace({
                workspace: {
                    projects: [],
                    generators: [{ source: "local", path: "generators/**" }],
                },
                projects: {
                    "generators/append/generator.omni.yaml": {
                        name: "append",
                        inputs: [
                            { type: "string", name: "entry", message: "Entry" },
                        ],
                        targets: { registry: "@workspace/registry.txt" },
                        actions: [
                            {
                                type: "append-content",
                                target: "registry",
                                entries: [
                                    {
                                        pattern: "# ENTRIES",
                                        content: "{{ inputs.entry }}",
                                    },
                                ],
                            },
                        ],
                    },
                },
                files: {
                    "registry.txt": "# ENTRIES\n",
                    ".omni/sources/generator/.keep": "",
                    "sub/.keep": "",
                },
            });
            const { client } = await connectMcp({
                cwd: launchDir().cwd,
                shared: true,
            });
            await Promise.all(
                ["alpha", "beta", "gamma"].map((entry) =>
                    content(client, "generator_run", ws.path("sub"), {
                        name: "append",
                        output_dir: "out",
                        dry_run: false,
                        use_defaults: false,
                        save_session: false,
                        ignore_session: true,
                        input_values: { entry },
                    }),
                ),
            );
            for (const entry of ["alpha", "beta", "gamma"])
                expect(ws.read("registry.txt")).toContain(entry);
        });

        it("keeps subscriber-backed task logs and environments independent", async () => {
            const workspaces = ["ALPHA", "BETA"].map((marker) =>
                makeWorkspace({
                    workspace: { projects: ["**"] },
                    projects: {
                        app: {
                            name: "app",
                            tasks: {
                                build: `node -e "console.log(process.env.SHARED_LOG_MARK)"`,
                            },
                        },
                    },
                    files: { ".env": `SHARED_LOG_MARK=${marker}\n` },
                }),
            );
            const { client } = await connectMcp({
                cwd: launchDir().cwd,
                shared: true,
            });
            const results = await Promise.all(
                workspaces.map((ws) =>
                    content(client, "task_run", ws.cwd, {
                        tasks: ["build"],
                        include_logs: true,
                    }),
                ),
            );
            expect(JSON.stringify(results[0])).toContain("ALPHA");
            expect(JSON.stringify(results[0])).not.toContain("BETA");
            expect(JSON.stringify(results[1])).toContain("BETA");
            expect(JSON.stringify(results[1])).not.toContain("ALPHA");
        });

        it.skipIf(spawnSync("deno", ["--version"]).status !== 0)(
            "executes shared tools in the supplied subdirectory and retains bound root defaults",
            async () => {
                const ws = makeWorkspace({
                    workspace: {
                        projects: [],
                        tools: [{ source: "local", path: "tools/**" }],
                    },
                    files: {
                        "tools/writer/tool.omni.yaml":
                            "type: js\nname: writer\nentrypoint: ./index.mjs\n",
                        "tools/writer/index.mjs":
                            'export default async function(ctx) { await ctx.sys.fs.writeStringToFile("out.txt", "ok"); return {}; }',
                        "sub/.keep": "",
                    },
                });
                const shared = await connectMcp({
                    cwd: launchDir().cwd,
                    shared: true,
                });
                await content(shared.client, "tool_run", ws.path("sub"), {
                    name: "writer",
                });
                expect(ws.read("sub/out.txt")).toBe("ok");
                expect(ws.exists("out.txt")).toBe(false);
                const bound = await connectMcp({ cwd: ws.cwd });
                await bound.client.callTool({
                    name: "tool_run",
                    arguments: { name: "writer" },
                });
                expect(ws.read("out.txt")).toBe("ok");
            },
        );
    },
);
