/**
 * RFC 0024 F03.4 slice 0 proof: a canonical task is its raw (project, task)
 * pair. Project `a#b` with task `c` and project `a` with task `b#c` render the
 * same unescaped `a#b#c` label, which the base still uses as an identity in
 * context lookups, result maps, output routing and the cache digest.
 *
 * The first block pins an ordinary undecorated task (schema, label, cache
 * layout and the v1 digest). The second block is the collision fixture and its
 * control; both run as ordinary tests since the cutover landed.
 */

import { readFileSync, readdirSync, renameSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type Workspace } from "@/harness";

const NO_CONFIG_HASH = { key: { defaults: false, files: ["src/**"] } };

interface ResultItem {
    status: string;
    hash: string | null;
    task: {
        task_name: string;
        project_name: string;
        full_task_name: string;
        dependencies: string[];
    };
    cache_hit: boolean;
    details: { meta: Record<string, string> };
}

interface Names {
    abProject: string;
    abTask: string;
    aProject: string;
    aTask: string;
    abRef: string;
    aRef: string;
}

const COLLIDING: Names = {
    abProject: "a#b",
    abTask: "c",
    aProject: "a",
    aTask: "b#c",
    abRef: "a\\#b#c",
    aRef: "a#b\\#c",
};

// Same fixture with names that render distinct labels: the control that
// separates the identity collision from every other behavior under test.
const CONTROL: Names = {
    abProject: "ab",
    abTask: "c",
    aProject: "a",
    aTask: "bc",
    abRef: "ab#c",
    aRef: "a#bc",
};

function pairTasks(n: Names) {
    return {
        "p-ab": {
            name: n.abProject,
            tasks: {
                [n.abTask]: {
                    exec: 'echo "RUN-AB-C mark=$MARK arg=$ARG"',
                    env: { vars: { MARK: "env-ab-c", ARG: "default-ab-c" } },
                    meta: { who: "ab-c" },
                    cache: NO_CONFIG_HASH,
                },
            },
        },
        "p-a": {
            name: n.aProject,
            tasks: {
                [n.aTask]: {
                    exec: 'echo "RUN-A-BC mark=$MARK arg=$ARG"',
                    env: { vars: { MARK: "env-a-bc", ARG: "default-a-bc" } },
                    meta: { who: "a-bc" },
                    cache: NO_CONFIG_HASH,
                },
            },
        },
    };
}

function collisionWorkspace(n: Names = COLLIDING): Workspace {
    return makeWorkspace({
        workspace: { projects: ["**"] },
        projects: {
            ...pairTasks(n),
            app: {
                name: "app",
                tasks: {
                    // Existing escaped references address each pair.
                    x: { exec: 'echo "APP-X"', dependencies: [n.abRef] },
                    y: { exec: 'echo "APP-Y"', dependencies: [n.aRef] },
                },
            },
        },
        files: { "p-ab/src/in.txt": "one", "p-a/src/in.txt": "one" },
    });
}

async function runApp(
    ws: Workspace,
    extra: string[] = [],
): Promise<{ items: ResultItem[]; stderr: string; stdout: string }> {
    const resultPath = ws.path("result.json");
    const result = await runOmni(
        [
            "run",
            "x",
            "y",
            "-p",
            "app",
            "--output-logs",
            "all",
            "--result",
            resultPath,
            "--result-format",
            "json",
            ...extra,
        ],
        { cwd: ws.cwd },
    );
    expect(result).toHaveSucceeded();
    return {
        items: JSON.parse(readFileSync(resultPath, "utf8")) as ResultItem[],
        stderr: result.stderr,
        stdout: result.stdout,
    };
}

function cacheEntries(ws: Workspace): string[] {
    const out = ws.path(".omni", "cache");
    const found: string[] = [];
    for (const project of readdirSync(out)) {
        const dir = join(out, project, "output");
        try {
            for (const digest of readdirSync(dir)) {
                found.push(
                    `${project.length > 0 ? "<project>" : ""}/output/<digest>/${readdirSync(join(dir, digest)).sort().join("+")}`,
                );
            }
        } catch {
            // not a project directory (locks, db)
        }
    }
    return found.sort();
}

describe("+run @e2e (base behavior of an ordinary task is pinned)", () => {
    function ordinary(): Workspace {
        return makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "ordinary $MODE $PROFILE"',
                            args: { profile: "fast" },
                            env: { vars: { MODE: "release", PROFILE: "p" } },
                            cache: NO_CONFIG_HASH,
                        },
                    },
                },
            },
            files: { "app/src/a.txt": "a" },
        });
    }

    it("keeps its label, result schema and cache layout", async () => {
        const ws = ordinary();
        const resultPath = ws.path("result.json");
        const result = await runOmni(
            ["run", "build", "--result", resultPath, "--result-format", "json"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();

        const [item] = JSON.parse(readFileSync(resultPath, "utf8"));
        expect(Object.keys(item).sort()).toEqual([
            "cache_hit",
            "cache_status",
            "details",
            "elapsed",
            "exit_code",
            "hash",
            "status",
            "task",
            "tries",
        ]);
        expect(Object.keys(item.task).sort()).toEqual([
            "dependencies",
            "enabled",
            "full_task_name",
            "interactive",
            "persistent",
            "project_dir",
            "project_name",
            "retry",
            "task_exec",
            "task_name",
            "task_retry_exec",
        ]);
        expect(item.task.full_task_name).toBe("app#build");
        expect(item.task.dependencies).toEqual([]);
        expect(item.hash).toMatch(/^[A-Za-z0-9+/]{43}=$/);
        expect(cacheEntries(ws)).toEqual([
            "<project>/output/<digest>/cache.meta.bin+logs.cache",
        ]);
    });

    it("has the pinned v1 digest for controlled inputs", async () => {
        const ws = ordinary();
        const resultPath = ws.path("result.json");
        await runOmni(
            ["run", "build", "--result", resultPath, "--result-format", "json"],
            { cwd: ws.cwd },
        );
        const [item] = JSON.parse(readFileSync(resultPath, "utf8"));
        expect(item.hash).toBe("S9EO0EDusfY8WI5qn+/84yBJaa5rQ+jWMMQ3EcCgkDc=");
    });
});

const find = (items: ResultItem[], project: string, task: string) =>
    items.find(
        (i) => i.task.project_name === project && i.task.task_name === task,
    );

function behaviors(names: Names, test: typeof it): void {
    test("both pairs run once with their own env, meta and logs", async () => {
        const ws = collisionWorkspace(names);
        const { items, stdout, stderr } = await runApp(ws);
        const output = `${stdout}\n${stderr}`;

        expect(output).toContain("RUN-AB-C mark=env-ab-c");
        expect(output).toContain("RUN-A-BC mark=env-a-bc");
        expect(items).toHaveLength(4);
        expect(
            find(items, names.abProject, names.abTask)?.details.meta.who,
        ).toBe("ab-c");
        expect(find(items, names.aProject, names.aTask)?.details.meta.who).toBe(
            "a-bc",
        );
    });

    test("labels render the pair and consumers depend on the right one", async () => {
        const ws = collisionWorkspace(names);
        const { items } = await runApp(ws);

        expect(
            find(items, names.abProject, names.abTask)?.task.full_task_name,
        ).toBe(names.abRef);
        expect(
            find(items, names.aProject, names.aTask)?.task.full_task_name,
        ).toBe(names.aRef);
        expect(find(items, "app", "x")?.task.dependencies).toEqual([
            names.abRef,
        ]);
        expect(find(items, "app", "y")?.task.dependencies).toEqual([
            names.aRef,
        ]);
    });

    test("cache entries do not cross between the pairs", async () => {
        const ws = collisionWorkspace(names);
        const first = await runApp(ws);
        const hashes = first.items.map((i) => i.hash);
        expect(new Set(hashes).size).toBe(4);

        const second = await runApp(ws, ["--output-cached-logs", "all"]);
        const output = `${second.stdout}\n${second.stderr}`;
        expect(second.items).toHaveLength(4);
        expect(second.items.every((i) => i.cache_hit)).toBe(true);
        expect(output).toContain("RUN-AB-C mark=env-ab-c");
        expect(output).toContain("RUN-A-BC mark=env-a-bc");
    });

    test("changing one pair re-runs only its own consumer", async () => {
        const ws = collisionWorkspace(names);
        await runApp(ws);

        ws.write("p-ab/src/in.txt", "changed");

        const second = await runApp(ws);
        const hit = (p: string, t: string) =>
            find(second.items, p, t)?.cache_hit;
        expect(hit(names.abProject, names.abTask)).toBe(false);
        expect(hit(names.aProject, names.aTask)).toBe(true);
        expect(hit("app", "x")).toBe(false);
        expect(hit("app", "y")).toBe(true);
    });
}

describe("+run @e2e (control: distinct labels behave independently)", () => {
    behaviors(CONTROL, it);
});

describe("+run @e2e (formerly colliding pairs stay independent)", () => {
    behaviors(COLLIDING, it);
});

describe("+run @e2e (same-project tasks keep their own cached logs)", () => {
    // Base defect found by the proof, independent of label collisions:
    // `cache_many` re-associates logs through a project-only map, so a
    // cache hit replays one task's logs for its sibling.
    it("replays each task's own logs on a cache hit", async () => {
        const ws = collisionWorkspace(CONTROL);
        await runApp(ws);
        const second = await runApp(ws, ["--output-cached-logs", "all"]);
        const output = `${second.stdout}\n${second.stderr}`;

        expect(output).toContain("APP-X");
        expect(output).toContain("APP-Y");
    });
});

const BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function base58(bytes: Buffer): string {
    let n = BigInt(`0x${bytes.toString("hex") || "0"}`);
    let out = "";
    while (n > 0n) {
        out = BASE58[Number(n % 58n)] + out;
        n /= 58n;
    }
    for (const byte of bytes) {
        if (byte !== 0) break;
        out = `1${out}`;
    }
    return out;
}

function parityWorkspace(opts: { appDependsOnLib: boolean }): Workspace {
    const cache = { key: { defaults: false, files: ["src/**"] } };
    return makeWorkspace({
        workspace: { projects: ["**"] },
        projects: {
            lib: { name: "lib", tasks: { gen: { exec: "echo gen", cache } } },
            app: {
                name: "app",
                tasks: {
                    x: {
                        exec: "echo x",
                        cache,
                        ...(opts.appDependsOnLib
                            ? { dependencies: ["lib#gen"] }
                            : {}),
                    },
                },
            },
        },
        files: { "lib/src/a.txt": "a", "app/src/b.txt": "b" },
    });
}

async function runResult(ws: Workspace, args: string[]): Promise<ResultItem[]> {
    const resultPath = ws.path(`result-${args.join("_")}.json`);
    const result = await runOmni(
        ["run", ...args, "--result", resultPath, "--result-format", "json"],
        { cwd: ws.cwd },
    );
    expect(result).toHaveSucceeded();
    return JSON.parse(readFileSync(resultPath, "utf8")) as ResultItem[];
}

describe("+run @e2e (v1 digest parity across run, dry-run, ignore-dependencies and cache)", () => {
    const hashOf = (items: ResultItem[], project: string, task: string) =>
        find(items, project, task)?.hash;

    it("dry-run computes the digest a real run records, cold and warm", async () => {
        const ws = parityWorkspace({ appDependsOnLib: true });
        const dry = await runResult(ws, ["x", "-p", "app", "--dry-run"]);
        const real = await runResult(ws, ["x", "-p", "app"]);
        const warmDry = await runResult(ws, ["x", "-p", "app", "--dry-run"]);

        const pairs: [string, string][] = [
            ["lib", "gen"],
            ["app", "x"],
        ];
        for (const [project, task] of pairs) {
            expect(hashOf(dry, project, task)).toBeTruthy();
            expect(hashOf(real, project, task)).toBe(
                hashOf(dry, project, task),
            );
            expect(hashOf(warmDry, project, task)).toBe(
                hashOf(real, project, task),
            );
        }
    });

    it("--ignore-dependencies yields the digest of the same task authored without dependencies", async () => {
        const withDeps = parityWorkspace({ appDependsOnLib: true });
        const ignored = await runResult(withDeps, [
            "x",
            "-p",
            "app",
            "--ignore-dependencies",
        ]);
        const without = parityWorkspace({ appDependsOnLib: false });
        const plain = await runResult(without, ["x", "-p", "app"]);
        const normal = await runResult(withDeps, ["x", "-p", "app"]);

        expect(ignored).toHaveLength(1);
        expect(hashOf(ignored, "app", "x")).toBe(hashOf(plain, "app", "x"));
        expect(hashOf(ignored, "app", "x")).not.toBe(
            hashOf(normal, "app", "x"),
        );
    });

    it("the collector digest is the cache output address and stale observation agrees", async () => {
        const ws = parityWorkspace({ appDependsOnLib: true });
        const items = await runResult(ws, ["x", "-p", "app"]);
        const digests = items.map((i) =>
            base58(Buffer.from(i.hash as string, "base64")),
        );

        const out = ws.path(".omni", "cache");
        const addressed = new Set<string>();
        for (const project of readdirSync(out)) {
            try {
                for (const d of readdirSync(join(out, project, "output"))) {
                    addressed.add(d);
                }
            } catch {
                // not a project directory
            }
        }
        expect([...addressed].sort()).toEqual([...digests].sort());

        const stale = await runOmni(["cache", "prune", "--stale", "-d"], {
            cwd: ws.cwd,
        });
        expect(stale).toHaveSucceeded();
        expect(`${stale.stdout}\n${stale.stderr}`).not.toContain("Project:");

        ws.write("app/src/b.txt", "changed");
        const afterChange = await runOmni(["cache", "prune", "--stale", "-d"], {
            cwd: ws.cwd,
        });
        const text = `${afterChange.stdout}\n${afterChange.stderr}`;
        expect(text).toContain("Project: app");
        expect(text).not.toContain("Project: lib");
    });

    it("stale observation keeps both formerly colliding pairs fresh and flags only the changed one", async () => {
        const ws = collisionWorkspace(COLLIDING);
        await runApp(ws);
        const fresh = await runOmni(["cache", "prune", "--stale", "-d"], {
            cwd: ws.cwd,
        });
        expect(`${fresh.stdout}\n${fresh.stderr}`).not.toContain("Project:");

        ws.write("p-ab/src/in.txt", "changed");
        const changed = await runOmni(["cache", "prune", "--stale", "-d"], {
            cwd: ws.cwd,
        });
        const text = `${changed.stdout}\n${changed.stderr}`;
        expect(text).toContain("Project: a#b");
        expect(text).toContain("  Task: c");
        expect(text).not.toContain("Task: b#c");
    });

    it("cache prune prints the whole raw task name containing a hash", async () => {
        const ws = collisionWorkspace(COLLIDING);
        await runApp(ws);
        const dry = await runOmni(
            ["cache", "prune", "-d", "--project", "a", "--task", "b#c"],
            { cwd: ws.cwd },
        );
        const text = `${dry.stdout}\n${dry.stderr}`;
        expect(text).toContain("Project: a");
        expect(text).toContain("Task: b#c");
    });
});

describe("+hash @e2e (workspace input union is per exact project)", () => {
    it("project `a` does not inherit the inputs of project `a#b`", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "p-a": {
                    name: "a",
                    tasks: {
                        t: {
                            exec: "echo t",
                            cache: {
                                key: { defaults: false, files: ["src/**"] },
                            },
                        },
                    },
                },
                "p-ab": {
                    name: "a#b",
                    tasks: {
                        c: {
                            exec: "echo c",
                            cache: {
                                key: { defaults: false, files: ["other/**"] },
                            },
                        },
                    },
                },
            },
            files: {
                "p-a/src/in.txt": "one",
                "p-a/other/in.txt": "one",
                "p-ab/other/in.txt": "one",
            },
        });
        const hash = async () => {
            const r = await runOmni(["hash", "-r", "workspace"], {
                cwd: ws.cwd,
            });
            expect(r).toHaveSucceeded();
            return r.stdout;
        };
        const before = await hash();
        // `other/**` belongs to project `a#b` only; project `a` must not hash it.
        ws.write("p-a/other/in.txt", "changed");
        expect(await hash()).toBe(before);
        ws.write("p-ab/other/in.txt", "changed");
        expect(await hash()).not.toBe(before);
    });
});

describe("+run @e2e (empty and NUL names are rejected at ingress)", () => {
    const run = async (projects: Record<string, unknown>) => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: projects as never,
        });
        return runOmni(["run", "ok", "--task-pattern", "*"], { cwd: ws.cwd });
    };

    it("rejects an empty project name, naming the project directory", async () => {
        const result = await run({
            p: { name: "", tasks: { ok: "echo ok" } },
        });
        expect(result).toHaveFailed();
        expect(`${result.stdout}\n${result.stderr}`).toContain(
            'invalid project name ""',
        );
    });

    it("rejects an empty task name, naming the project", async () => {
        const result = await run({
            p: { name: "p", tasks: { ok: "echo ok", "": "echo empty" } },
        });
        expect(result).toHaveFailed();
        expect(`${result.stdout}\n${result.stderr}`).toContain(
            "invalid task name \"\" in project 'p'",
        );
    });

    it("rejects a NUL in a task name", async () => {
        const result = await run({
            p: { name: "p", tasks: { ok: "echo ok", "t\u0000u": "echo nul" } },
        });
        expect(result).toHaveFailed();
        expect(`${result.stdout}\n${result.stderr}`).toContain(
            "it contains a NUL character",
        );
    });
});

describe("+run @e2e (a digest-addressed entry from another recipe is never reused)", () => {
    it("misses, rebuilds under the v1 address and leaves the foreign entry untouched", async () => {
        const ws = parityWorkspace({ appDependsOnLib: false });
        const first = await runResult(ws, ["x", "-p", "app"]);
        const digest = base58(Buffer.from(first[0]?.hash as string, "base64"));

        const out = ws.path(".omni", "cache");
        const projectDir = readdirSync(out)
            .map((p) => join(out, p, "output"))
            .find((dir) => {
                try {
                    return readdirSync(dir).includes(digest);
                } catch {
                    return false;
                }
            }) as string;
        // Move the v1 entry to a different address, as a legacy digest would.
        const foreign = "LegacyRecipeDigestFor";
        renameSync(join(projectDir, digest), join(projectDir, foreign));

        const second = await runResult(ws, ["x", "-p", "app"]);
        expect(second[0]?.cache_hit).toBe(false);
        expect(second[0]?.hash).toBe(first[0]?.hash);
        expect(readdirSync(projectDir).sort()).toEqual(
            [digest, foreign].sort(),
        );
    });
});
