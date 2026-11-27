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
const HIT_GREET = "Cache hit for task 'app#greet'";

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
    /** A setting scoped to the `build` task only (`meta.note`). */
    buildNote?: string;
    /** Sibling tasks `lint` pulls in (`with`). */
    lintWith?: string[];
    /** Project-level `env.vars.X` (the app's own definition). */
    projectEnv?: string;
    /** `env.vars.X` in the base file the app extends. */
    baseEnv?: string;
    /** Workspace-level `env.vars.X`. */
    workspaceEnv?: string;
    /** Leave `cache.key.defaults` at its default (true): config files are inputs. */
    defaultInputs?: boolean;
}

function baseFile(options: Options) {
    return {
        name: "base",
        base: true,
        ...(options.baseDescription
            ? { description: options.baseDescription }
            : {}),
        meta: { flavor: options.baseFlavor ?? "one" },
        ...(options.baseEnv ? { env: { vars: { X: options.baseEnv } } } : {}),
    };
}

function spec(options: Options = {}): WorkspaceSpec {
    // The config files are not inputs (`defaults: false`) and the only file
    // input never changes, so no content can stand in for the definition.
    const cache = {
        key: options.defaultInputs
            ? { files: ["src/**"] }
            : { defaults: false, files: ["src/**"] },
    };
    return {
        workspace: {
            projects: ["**"],
            ...(options.workspaceEnv
                ? { env: { vars: { X: options.workspaceEnv } } }
                : {}),
        },
        projects: {
            "base.omni.yaml": baseFile(options),
            app: {
                name: "app",
                extends: ["../base.omni.yaml"],
                ...(options.appFlavor
                    ? { meta: { flavor: options.appFlavor } }
                    : {}),
                ...(options.projectEnv
                    ? { env: { vars: { X: options.projectEnv } } }
                    : {}),
                tasks: {
                    build: {
                        exec: "node marker.js {{ meta.flavor }}",
                        cache,
                        ...(options.buildNote
                            ? { meta: { note: options.buildNote } }
                            : {}),
                    },
                    lint: {
                        exec: "node marker.js lint",
                        cache,
                        ...(options.lintWith ? { with: options.lintWith } : {}),
                    },
                    side: {
                        exec: "node marker.js side",
                        cache,
                    },
                    greet: {
                        exec: "node marker.js {{ args.k }}",
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

function appFile(options: Options) {
    return spec(options).projects?.app as NonNullable<
        WorkspaceSpec["projects"]
    >[string];
}

function workspaceFile(options: Options) {
    return spec(options).workspace as NonNullable<WorkspaceSpec["workspace"]>;
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

    it("guard: a task keeps its entry when only another task's own setting changes", async () => {
        // F06 includes the full merged project and task `meta` of the task
        // itself, so the guard edits a setting scoped to `build` alone: `lint`
        // does not inherit it and its own definition is unchanged.
        const ws = makeWorkspace(spec({ buildNote: "one" }));
        await run(ws, "lint");

        ws.write("app/project.omni.yaml", appFile({ buildNote: "two" }));
        const second = await run(ws, "lint");

        expect(second).toHaveStderrContaining(HIT_LINT);
        expect(runs(ws)).toEqual(["lint"]);
    });

    it("re-runs a task when only an invocation argument changed (Q1)", async () => {
        const ws = makeWorkspace(spec());
        const first = await runOmni(["run", "greet", "-a", "k=v1"], {
            cwd: ws.cwd,
        });
        expect(first).toHaveSucceeded();
        const second = await runOmni(["run", "greet", "-a", "k=v2"], {
            cwd: ws.cwd,
        });

        expect(second).toHaveSucceeded();
        expect(second.stderr).not.toContain(HIT_GREET);
        expect(runs(ws)).toEqual(["v1", "v2"]);
    });

    it("guard: the same invocation argument hits the cache (Q1)", async () => {
        const ws = makeWorkspace(spec());
        const first = await runOmni(["run", "greet", "-a", "k=v1"], {
            cwd: ws.cwd,
        });
        expect(first).toHaveSucceeded();
        const second = await runOmni(["run", "greet", "-a", "k=v1"], {
            cwd: ws.cwd,
        });

        expect(second).toHaveStderrContaining(HIT_GREET);
        expect(runs(ws)).toEqual(["v1"]);
    });

    it("re-runs a task whose `with` sibling set changed (Q2)", async () => {
        const ws = makeWorkspace(spec());
        await run(ws, "lint");

        ws.write("app/project.omni.yaml", appFile({ lintWith: ["side"] }));
        const second = await run(ws, "lint");

        expect(second.stderr).not.toContain(HIT_LINT);
    });

    it("re-runs a task whose inherited project env definition changed (Q3)", async () => {
        const ws = makeWorkspace(spec({ baseEnv: "one" }));
        await run(ws, "lint");

        ws.write("base.omni.yaml", baseFile({ baseEnv: "two" }));
        const second = await run(ws, "lint");

        expect(second.stderr).not.toContain(HIT_LINT);
    });

    it("re-runs a task whose project env definition changed (Q3)", async () => {
        const ws = makeWorkspace(spec({ projectEnv: "one" }));
        await run(ws, "lint");

        ws.write("app/project.omni.yaml", appFile({ projectEnv: "two" }));
        const second = await run(ws, "lint");

        expect(second.stderr).not.toContain(HIT_LINT);
    });

    it("re-runs a task whose workspace env definition changed (Q3)", async () => {
        const ws = makeWorkspace(spec({ workspaceEnv: "one" }));
        await run(ws, "lint");

        ws.write("workspace.omni.yaml", workspaceFile({ workspaceEnv: "two" }));
        const second = await run(ws, "lint");

        expect(second.stderr).not.toContain(HIT_LINT);
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

    it("the final key does not depend on the checkout path (defaults on)", async () => {
        const a = makeWorkspace(spec({ defaultInputs: true }));
        const b = makeWorkspace(spec({ defaultInputs: true }));
        expect(a.cwd).not.toBe(b.cwd);

        expect(await projectHash(a, "build")).toBe(
            await projectHash(b, "build"),
        );
    });

    it("the final key changes when a default config input the task reads changes (defaults on)", async () => {
        const ws = makeWorkspace(
            spec({ defaultInputs: true, baseDescription: "one" }),
        );
        const before = await projectHash(ws, "build");

        // A presentation-only edit: only the file content differs, and the
        // default inputs are what put it in the key.
        ws.write("base.omni.yaml", baseFile({ baseDescription: "two" }));

        expect(await projectHash(ws, "build")).not.toBe(before);
    });
});
