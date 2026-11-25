/**
 * RFC 0024 F03.4 slices 1-3: authored task aliases and canonical selection.
 *
 * The thin proof fixture: one alias and one mixed exact/pattern request go
 * through every live surface (run, hash, cache eligibility, MCP) and must plan
 * one canonical execution. Stored cache names stay canonical (slice 4 tests
 * live in the cache specs).
 */

import { Validator } from "@cfworker/json-schema";
import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    runOmni,
    type WorkspaceSpec,
} from "@/harness";

function spec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            web: {
                name: "web",
                tasks: {
                    "build-native": 'echo "native"',
                    compile: { alias: "build-native" },
                    lint: 'echo "lint"',
                },
            },
            app: {
                name: "app",
                dependencies: ["web"],
                tasks: {
                    bundle: {
                        exec: 'echo "bundle"',
                        dependencies: ["web#compile"],
                    },
                    // A real task that shares the alias's name elsewhere.
                    compile: { exec: 'echo "app-compile"' },
                },
            },
            esc: {
                name: "esc",
                tasks: { "a\\#b": 'echo "hash-name"' },
            },
        },
    };
}

function count(text: string, needle: string): number {
    return text.split(needle).length - 1;
}

describe("+run @e2e (aliases and canonical selection)", () => {
    it("an alias runs its canonical task once", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            ["run", "compile", "-p", "web", "--no-cache"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(count(result.stderr, "Executed task 'web#build-native'")).toBe(
            1,
        );
        expect(result.stderr).not.toContain("web#compile");
    });

    it("alias, canonical name and a matching pattern select one execution", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            [
                "run",
                "compile",
                "build-native",
                "--task-pattern",
                "comp*",
                "--task-pattern",
                "build-*",
                "-p",
                "web",
                "--output-logs",
                "all",
            ],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(count(result.stderr, "Executed task 'web#build-native'")).toBe(
            1,
        );
        expect(result.stderr).not.toContain("web#lint");
    });

    it("mixed exact (alias) and pattern selectors run the union", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            [
                "run",
                "compile",
                "--task-pattern",
                "li*",
                "-p",
                "web",
                "--output-logs",
                "all",
            ],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(count(result.stderr, "Executed task 'web#build-native'")).toBe(
            1,
        );
        expect(count(result.stderr, "Executed task 'web#lint'")).toBe(1);
    });

    it("a pattern matching an alias selects the canonical task", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            ["run", "--task-pattern", "comp*", "-p", "web"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("web#build-native");
    });

    it("a dependency on an alias becomes an edge to the canonical task", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(["run", "bundle", "--no-cache"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        const native = result.stderr.indexOf(
            "Executed task 'web#build-native'",
        );
        const bundle = result.stderr.indexOf("Executed task 'app#bundle'");
        expect(native).toBeGreaterThanOrEqual(0);
        expect(bundle).toBeGreaterThan(native);
    });

    it("an own-project alias in dependencies and with loads and runs once, in order", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                solo: {
                    name: "solo",
                    tasks: {
                        "prep-real": 'echo "prep"',
                        prep: { alias: "prep-real" },
                        "side-real": 'echo "side"',
                        side: { alias: "side-real" },
                        main: {
                            exec: 'echo "main"',
                            dependencies: ["prep", "solo#prep"],
                            with: ["side", "side-real"],
                        },
                    },
                },
            },
        });
        const result = await runOmni(["run", "main", "--no-cache"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(count(result.stderr, "Executed task 'solo#prep-real'")).toBe(1);
        expect(count(result.stderr, "Executed task 'solo#side-real'")).toBe(1);
        expect(count(result.stderr, "Executed task 'solo#main'")).toBe(1);
        expect(result.stderr).not.toContain("solo#prep'");
        expect(result.stderr).not.toContain("solo#side'");
        const prep = result.stderr.indexOf("Executed task 'solo#prep-real'");
        const main = result.stderr.indexOf("Executed task 'solo#main'");
        expect(prep).toBeGreaterThanOrEqual(0);
        expect(main).toBeGreaterThan(prep);
    });

    it("a project qualifier scopes the name to one project", async () => {
        const ws = makeWorkspace(spec());
        const web = await runOmni(["run", "web#compile"], { cwd: ws.cwd });
        expect(web).toHaveSucceeded();
        expect(web).toHaveStderrContaining("web#build-native");
        expect(web.stderr).not.toContain("app#compile");
        const app = await runOmni(["run", "app#compile"], { cwd: ws.cwd });
        expect(app).toHaveSucceeded();
        expect(app).toHaveStderrContaining("app#compile");
        expect(app.stderr).not.toContain("web#build-native");
        // Unqualified: the alias in `web` and the task in `app` both match.
        const both = await runOmni(["run", "compile"], { cwd: ws.cwd });
        expect(both).toHaveSucceeded();
        expect(both).toHaveStderrContaining("web#build-native");
        expect(both).toHaveStderrContaining("app#compile");
    });

    it("a missing exact name is not hidden by a successful glob", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            ["run", "nope", "--task-pattern", "li*", "-p", "web"],
            { cwd: ws.cwd },
        );
        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `nope`");
    });

    it("wildcards in an exact name stay literal, even for an alias prefix", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(["run", "comp*", "-p", "web"], {
            cwd: ws.cwd,
        });
        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `comp*`");
    });

    it("escaped `#` in a task name is selectable by its escaped spelling", async () => {
        const ws = makeWorkspace(spec());
        for (const selector of ["a\\#b", "esc#a\\#b"]) {
            const result = await runOmni(["run", selector], { cwd: ws.cwd });
            expect(result).toHaveSucceeded();
            expect(result).toHaveStderrContaining("esc#a\\#b");
        }
    });

    it("rejects unsupported reserved syntax in a selector", async () => {
        const ws = makeWorkspace(spec());
        for (const bad of ["^compile", "native::build", "a#b#c"]) {
            const result = await runOmni(["run", bad], { cwd: ws.cwd });
            expect(result).not.toHaveSucceeded();
            expect(result.stderr).toContain("invalid task selector");
        }
    });
});

describe("+hashing @e2e (aliases select the same canonical task)", () => {
    async function hash(cwd: string, ...selectors: string[]) {
        const result = await runOmni(
            ["hash", "-r", "project", "web", ...selectors],
            { cwd },
        );
        expect(result).toHaveSucceeded();
        return result.stdout.trim();
    }

    it("alias, canonical name and a pattern give one digest", async () => {
        const ws = makeWorkspace(spec());
        const canonical = await hash(ws.cwd, "-t", "build-native");
        expect(await hash(ws.cwd, "-t", "compile")).toBe(canonical);
        expect(await hash(ws.cwd, "-t", "web#compile")).toBe(canonical);
        expect(await hash(ws.cwd, "-t", "compile", "-t", "build-native")).toBe(
            canonical,
        );
        expect(await hash(ws.cwd, "--task-pattern", "comp*")).toBe(canonical);
    });

    it("rejects a qualifier that names another project", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            ["hash", "-r", "project", "web", "-t", "app#compile"],
            { cwd: ws.cwd },
        );
        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("names another project");
    });

    it("a dependency-only task is not folded into the aggregate", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                web: {
                    name: "web",
                    tasks: {
                        prep: 'echo "prep"',
                        build: { exec: 'echo "b"', dependencies: ["prep"] },
                        quick: { alias: "build" },
                    },
                },
            },
        });
        const both = await hash(ws.cwd, "-t", "build", "-t", "prep");
        const only = await hash(ws.cwd, "-t", "quick");
        expect(only).not.toBe(both);
    });
});

describe("+cache @e2e (eligibility selects through aliases)", () => {
    it("`cache eligibility -t <alias>` reports the canonical task", async () => {
        const ws = makeWorkspace(spec());
        const result = await runOmni(
            ["cache", "eligibility", "-t", "compile", "-p", "web", "--json"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        const data = JSON.parse(result.stdout) as {
            tasks: Array<{ task: string }>;
        };
        expect(data.tasks.map((t) => t.task)).toEqual(["web#build-native"]);
    });
});

describe("+mcp @mcp @cli (aliases)", { tags: ["mcp", "caching"] }, () => {
    it("task_run, hash_project and cache_eligibility agree with the CLI", async () => {
        const ws = makeWorkspace(spec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const run = await client.callTool({
            name: "task_run",
            arguments: {
                tasks: ["compile"],
                task_patterns: ["li*"],
                project: ["web"],
                dry_run: true,
            },
        });
        expect(run.isError).not.toBe(true);

        const hashOf = async (tasks: string[]) => {
            const r = await client.callTool({
                name: "hash_project",
                arguments: { name: "web", tasks },
            });
            return (r.structuredContent as { hash: string }).hash;
        };
        const canonical = await hashOf(["build-native"]);
        expect(await hashOf(["compile"])).toBe(canonical);
        expect(await hashOf(["compile", "build-native"])).toBe(canonical);

        const elig = await client.callTool({
            name: "cache_eligibility",
            arguments: { tasks: ["compile"], project: ["web"] },
        });
        const data = elig.structuredContent as {
            tasks: Array<{ task: string }>;
        };
        expect(data.tasks.map((t) => t.task)).toEqual(["web#build-native"]);

        await expect(
            client.callTool({
                name: "hash_project",
                arguments: { name: "web", tasks: ["app#compile"] },
            }),
        ).rejects.toThrow(/another project/);
    });

    it("task_run executes the canonical task once for an alias plus canonical", async () => {
        const ws = makeWorkspace(spec());
        const { client } = await connectMcp({ cwd: ws.cwd });
        const ok = await client.callTool({
            name: "task_run",
            arguments: {
                tasks: ["compile", "build-native"],
                task_patterns: ["comp*"],
                project: ["web"],
                no_cache: true,
            },
        });
        const data = ok.structuredContent as {
            ok: boolean;
            results: Array<{ project: string; task: string }>;
        };
        expect(data.ok).toBe(true);
        expect(data.results.map((r) => `${r.project}#${r.task}`)).toEqual([
            "web#build-native",
        ]);
    });
});

describe("+config @e2e (authored schema and loader agree)", () => {
    async function schema(cwd: string) {
        const result = await runOmni(["config", "schema", "project"], {
            cwd,
        });
        expect(result).toHaveSucceeded();
        return JSON.parse(result.stdout);
    }

    const accepted: Array<[string, Record<string, unknown>]> = [
        [
            "alias beside normal tasks",
            { name: "p", tasks: { t: "echo", a: { alias: "t" } } },
        ],
        [
            "alias inside a merge envelope",
            { name: "p", tasks: { merge: { a: { alias: "t" } } } },
        ],
        [
            "operator-named entries inside an explicit envelope",
            {
                name: "p",
                tasks: {
                    merge: { merge: "echo", replace: { alias: "merge" } },
                },
            },
        ],
        [
            "a task named merge as a short form",
            { name: "p", tasks: { merge: "echo", other: "echo" } },
        ],
    ];
    // A layer without `name` is valid for the schema (it may only be a base
    // or extended layer); the loader requires the name once composed.
    const schemaOnly: Array<Record<string, unknown>> = [
        { base: true, tasks: { t: "echo" } },
    ];
    const rejected: Array<[string, Record<string, unknown>]> = [
        [
            "a mixed alias and task object",
            {
                name: "p",
                tasks: { t: { exec: "echo", alias: "x" } },
            },
        ],
        [
            "a mixed operator envelope",
            { name: "p", tasks: { merge: { a: "echo" }, b: "echo" } },
        ],
        [
            "a non-string alias target",
            { name: "p", tasks: { a: { alias: 1 } } },
        ],
    ];

    it("accepts and rejects the same alias shapes as the loader", async () => {
        const ws = makeWorkspace({ workspace: { projects: ["**"] } });
        const validator = new Validator(await schema(ws.cwd), "2020-12");
        const load = async (doc: Record<string, unknown>) => {
            const w = makeWorkspace({
                workspace: { projects: ["**"] },
                projects: { p: { ...doc } },
            });
            // Loader acceptance only: a dangling alias target is a separate,
            // post-composition check.
            return runOmni(["project", "list"], { cwd: w.cwd });
        };
        for (const [label, doc] of accepted) {
            expect(validator.validate(doc).valid, `schema: ${label}`).toBe(
                true,
            );
            const loaded = await load(
                label.includes("alias beside") ||
                    label.includes("merge envelope") ||
                    label.includes("operator-named")
                    ? withTargets(doc)
                    : doc,
            );
            expect(loaded, `loader: ${label}`).toHaveSucceeded();
        }
        for (const doc of schemaOnly) {
            expect(validator.validate(doc).valid).toBe(true);
        }
        for (const [label, doc] of rejected) {
            expect(validator.validate(doc).valid, `schema: ${label}`).toBe(
                false,
            );
            const loaded = await load(doc);
            expect(loaded, `loader: ${label}`).not.toHaveSucceeded();
        }
    });

    function withTargets(doc: Record<string, unknown>) {
        const tasks = doc.tasks as Record<string, unknown>;
        const extra: Record<string, unknown> =
            "merge" in tasks && typeof tasks.merge === "object"
                ? { merge: { ...(tasks.merge as object), t: "echo" } }
                : { ...tasks, t: "echo" };
        return { ...doc, tasks: extra };
    }
});
