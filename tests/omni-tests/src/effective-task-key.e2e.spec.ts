/**
 * RFC 0024 F06.3 proof: the cache key follows the effective task.
 *
 * The collector hashes the raw command text, input file contents, configured
 * env values, args and dependency digests. A setting that a task inherits
 * through `extends` and that its command consumes (`{{ meta.flavor }}`) is none
 * of those: edit it in the base file and the raw command text, and with
 * `cache.key.defaults: false` every input, stay the same, while the rendered
 * command, and so the task's behavior, changes. Before F06.3 the second run
 * hits the cache and replays the old result.
 *
 * Tests marked "guard" pass on the base and must keep passing: presentation
 * and unrelated changes must not move a key. The others fail on the base.
 *
 * Portable: tasks run `node` scripts, no shell syntax.
 */

import { describe, expect, it } from "vitest";

import {
    makeWorkspace,
    runOmni,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";

const RAW_HASH_PATTERN = /^[A-Za-z0-9]{40,}$/;

const HIT_BUILD = "Cache hit for task 'app#build'";
const HIT_LINT = "Cache hit for task 'app#lint'";

/** Appends its first argument to `runs.txt`: one line per real execution. */
const MARKER = [
    'const fs = require("node:fs");',
    'fs.appendFileSync("runs.txt", `${process.argv[2] ?? ""}\\n`);',
    "",
].join("\n");

interface Options {
    baseFlavor?: string;
    baseDescription?: string;
    /** The app restates the flavor itself, masking the inherited one. */
    appFlavor?: string;
}

function baseFile(options: Options) {
    return {
        name: "base",
        base: true,
        ...(options.baseDescription
            ? { description: options.baseDescription }
            : {}),
        meta: { flavor: options.baseFlavor ?? "one" },
    };
}

function spec(options: Options = {}): WorkspaceSpec {
    // The config files are not inputs (`defaults: false`) and the only file
    // input never changes, so no content can stand in for the definition.
    const cache = { key: { defaults: false, files: ["src/**"] } };
    return {
        workspace: { projects: ["**"] },
        projects: {
            "base.omni.yaml": baseFile(options),
            app: {
                name: "app",
                extends: ["../base.omni.yaml"],
                ...(options.appFlavor
                    ? { meta: { flavor: options.appFlavor } }
                    : {}),
                tasks: {
                    build: {
                        exec: "node marker.js {{ meta.flavor }}",
                        cache,
                    },
                    lint: {
                        exec: "node marker.js lint",
                        cache,
                    },
                },
            },
        },
        files: {
            "app/marker.js": MARKER,
            "app/src/a.txt": "a\n",
        },
    };
}

function runs(ws: Workspace): string[] {
    return ws.exists("app/runs.txt")
        ? ws.read("app/runs.txt").split(/\r?\n/).filter(Boolean)
        : [];
}

async function run(ws: Workspace, task: string) {
    const result = await runOmni(["run", task], { cwd: ws.cwd });
    expect(result).toHaveSucceeded();
    return result;
}

async function projectHash(ws: Workspace, task: string): Promise<string> {
    const result = await runOmni(["hash", "-r", "project", "app", "-t", task], {
        cwd: ws.cwd,
    });
    expect(result).toHaveSucceeded();
    expect(result.stdout.trim()).toMatch(RAW_HASH_PATTERN);
    return result.stdout.trim();
}

describe("+cache @cache @e2e (F06.3 the key follows the effective task)", () => {
    it("control: an unchanged task hits the cache", async () => {
        const ws = makeWorkspace(spec());

        expect(runs(ws)).toEqual([]);
        await run(ws, "build");
        const second = await run(ws, "build");

        expect(second).toHaveStderrContaining(HIT_BUILD);
        expect(runs(ws)).toEqual(["one"]);
    });

    it("re-runs a task whose inherited setting changed in the base file", async () => {
        const ws = makeWorkspace(spec({ baseFlavor: "one" }));
        await run(ws, "build");

        ws.write("base.omni.yaml", baseFile({ baseFlavor: "two" }));
        const second = await run(ws, "build");

        expect(second.stderr).not.toContain(HIT_BUILD);
        expect(runs(ws)).toEqual(["one", "two"]);
    });

    it("`hash project` changes when an inherited setting changes", async () => {
        const ws = makeWorkspace(spec({ baseFlavor: "one" }));
        const before = await projectHash(ws, "build");

        ws.write("base.omni.yaml", baseFile({ baseFlavor: "two" }));

        expect(await projectHash(ws, "build")).not.toBe(before);
    });

    it("`cache prune --stale` sees the old entry as stale after the change", async () => {
        const ws = makeWorkspace(spec({ baseFlavor: "one" }));
        await run(ws, "build");

        ws.write("base.omni.yaml", baseFile({ baseFlavor: "two" }));
        const dry = await runOmni(["cache", "prune", "--stale", "--dry-run"], {
            cwd: ws.cwd,
        });

        expect(dry).toHaveSucceeded();
        expect(dry.stdout + dry.stderr).not.toContain(
            "No cache entries matched the given filters",
        );
        expect(dry).toOutputContaining("build");
    });

    it("guard: a task that does not read the changed setting keeps its entry", async () => {
        const ws = makeWorkspace(spec({ baseFlavor: "one" }));
        await run(ws, "lint");

        // `lint` never reads the flavor and its own definition is unchanged.
        ws.write("base.omni.yaml", baseFile({ baseFlavor: "two" }));
        const second = await run(ws, "lint");

        expect(second).toHaveStderrContaining(HIT_LINT);
        expect(runs(ws)).toEqual(["lint"]);
    });

    it("guard: a presentation-only change in the base file keeps the entry", async () => {
        const ws = makeWorkspace(spec({ baseFlavor: "one" }));
        await run(ws, "build");

        ws.write(
            "base.omni.yaml",
            baseFile({ baseFlavor: "one", baseDescription: "reworded" }),
        );
        const second = await run(ws, "build");

        expect(second).toHaveStderrContaining(HIT_BUILD);
        expect(runs(ws)).toEqual(["one"]);
    });

    it("guard: an explicit override masks an irrelevant inherited change", async () => {
        const ws = makeWorkspace(
            spec({ baseFlavor: "one", appFlavor: "mine" }),
        );
        await run(ws, "build");

        ws.write("base.omni.yaml", baseFile({ baseFlavor: "two" }));
        const second = await run(ws, "build");

        expect(second).toHaveStderrContaining(HIT_BUILD);
        expect(runs(ws)).toEqual(["mine"]);
    });
});
