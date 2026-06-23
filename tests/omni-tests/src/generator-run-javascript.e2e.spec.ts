/**
 * `run-javascript` generator action - exercises the bridge service runner end
 * to end: `omni generator run` spawns the vendored JS bridge, hands the script
 * its context (`sys`/`log`/`isDryRun`/`data`), and the script's file-system
 * mutations flow through the same transactional overlay as the rest of the
 * generator. Pinned to `crates/omni_generator/src/action_handlers/run_javascript.rs`
 * and `crates/omni_generator/src/script_runner.rs`.
 *
 * These tests require a JS runtime (node/bun/deno) on PATH; the runner
 * auto-detects one.
 */

import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/** Whether a JS runtime binary is callable on PATH (for gating tests). */
function runtimeAvailable(bin: string): boolean {
    try {
        return spawnSync(bin, ["--version"], { stdio: "ignore" }).status === 0;
    } catch {
        return false;
    }
}

/**
 * A generator script that writes `data.message` to `data.target` through the
 * bridge-backed `sys`. `target` is relative, so it resolves against the
 * generator's (transactional) working directory - the workspace root.
 */
const WRITE_SCRIPT = `export default async function (ctx) {
    const { target, message } = ctx.data;
    await ctx.sys.fs.writeStringToFile(target, message);
}
`;

/**
 * A workspace whose `jsgen` generator runs {@link WRITE_SCRIPT} via a single
 * `run-javascript` action. `data.message` is templated so the test can prove
 * `data` is rendered against the generator context before reaching the script.
 */
function jsGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/jsgen/generator.omni.yaml": {
                name: "jsgen",
                description: "runs a JS generator script",
                vars: { who: "world" },
                actions: [
                    {
                        type: "run-javascript",
                        script: "gen.mjs",
                        data: {
                            target: "from-js.txt",
                            message: "Hello {{ vars.who }}",
                        },
                    },
                ],
            },
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "generators/jsgen/gen.mjs": WRITE_SCRIPT,
        },
    };
}

describe("+generator @e2e (run-javascript)", {
    tags: ["generator"],
}, () => {
    it("runs a script through the bridge runner and commits its sys writes", async () => {
        const ws = makeWorkspace(jsGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "jsgen",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // The script wrote via `ctx.sys`, and the (non-dry) run committed it.
        // `data.message` was templated against the generator's `vars`.
        expect(ws.read("out/from-js.txt")).toBe("Hello world");
    });

    it("commits nothing under --dry-run", async () => {
        const ws = makeWorkspace(jsGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "jsgen",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // The script still ran, but the transaction was rolled back.
        expect(ws.exists("from-js.txt")).toBe(false);
    });

    it("shares one runner across nested run-generator -> run-javascript", async () => {
        // `parent` runs its own script AND delegates to `child` (which also
        // runs a script). A single, shared JS runner must service both.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: [{ source: "local", path: "generators/**" }],
            },
            projects: {
                "generators/child/generator.omni.yaml": {
                    name: "child",
                    description: "child generator",
                    actions: [
                        {
                            type: "run-javascript",
                            script: "gen.mjs",
                            data: { target: "child.txt", message: "child" },
                        },
                    ],
                },
                "generators/parent/generator.omni.yaml": {
                    name: "parent",
                    description: "parent generator",
                    actions: [
                        {
                            type: "run-javascript",
                            script: "gen.mjs",
                            data: { target: "parent.txt", message: "parent" },
                        },
                        { type: "run-generator", generator: "child" },
                    ],
                },
            },
            files: {
                ".omni/sources/generator/.keep": "",
                "generators/child/gen.mjs": WRITE_SCRIPT,
                "generators/parent/gen.mjs": WRITE_SCRIPT,
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
        expect(ws.read("out/parent.txt")).toBe("parent");
        expect(ws.read("out/child.txt")).toBe("child");
    });

    it("honors per-action runtimes, spawning one runner each", {
        timeout: 90_000,
    }, async (ctx) => {
        // Each `run-javascript` action picks its own runtime; distinct runtimes
        // get distinct processes. Gated so it only runs where both exist.
        if (!runtimeAvailable("node") || !runtimeAvailable("bun")) {
            ctx.skip();
            return;
        }

        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: [{ source: "local", path: "generators/**" }],
            },
            projects: {
                "generators/multi/generator.omni.yaml": {
                    name: "multi",
                    description: "runs scripts on two runtimes",
                    actions: [
                        {
                            type: "run-javascript",
                            runtime: "node",
                            script: "gen.mjs",
                            data: { target: "on-node.txt", message: "node" },
                        },
                        {
                            type: "run-javascript",
                            runtime: "bun",
                            script: "gen.mjs",
                            data: { target: "on-bun.txt", message: "bun" },
                        },
                    ],
                },
            },
            files: {
                ".omni/sources/generator/.keep": "",
                "generators/multi/gen.mjs": WRITE_SCRIPT,
            },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "multi",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/on-node.txt")).toBe("node");
        expect(ws.read("out/on-bun.txt")).toBe("bun");
    });

    it("should propagate errors from omni to the js script", async () => {
        // `parent` runs its own script AND delegates to `child` (which also
        // runs a script). A single, shared JS runner must service both.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: [{ source: "local", path: "generators/**" }],
            },
            projects: {
                "generators/js/generator.omni.yaml": {
                    name: "js",
                    description: "js generator",
                    actions: [
                        {
                            type: "run-javascript",
                            script: "gen.mjs",
                            data: { target: "child.txt", message: "child" },
                        },
                    ],
                },
            },
            files: {
                ".omni/sources/generator/.keep": "",
                "generators/js/gen.mjs": `
                export default async function (ctx) {
                    try {
                        await ctx.sys.fs.readFileAsString("does_not_exist.txt");
                    } catch (e) {
                        ctx.log.error(e);
                        throw e;
                    }
                }
                `,
            },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "js",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // The exact wording of the "file not found" OS error differs by platform:
        //   POSIX   → "No such file or directory"
        //   Windows → "The system cannot find the path specified."
        const fileNotFoundMsg =
            process.platform === "win32"
                ? "The system cannot find the path specified."
                : "No such file or directory";
        expect(result).toOutputContaining(fileNotFoundMsg);
    });
});
