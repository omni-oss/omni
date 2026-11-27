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

// A flat input keeps the hashed `@project/<path>` free of a path separator,
// which the collector renders natively (`\` on Windows), so pinned digests
// hold on every OS.
const FLAT_CONFIG_HASH = { key: { defaults: false, files: ["*.txt"] } };
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

/** A raw task name spelled as a declaration key or a live exact selector. */
function esc(name: string): string {
    return name.replace(/[\\#]/g, "\\$&");
}

function pairTasks(n: Names) {
    return {
        "p-ab": {
            name: n.abProject,
            tasks: {
                [esc(n.abTask)]: {
                    exec: 'echo "RUN-AB-C mark=$MARK arg={{ args.who }}"',
                    args: { who: "default-ab-c" },
                    env: { vars: { MARK: "env-ab-c" } },
                    meta: { who: "ab-c" },
                    cache: NO_CONFIG_HASH,
                },
            },
        },
        "p-a": {
            name: n.aProject,
            tasks: {
                [esc(n.aTask)]: {
                    exec: 'echo "RUN-A-BC mark=$MARK arg={{ args.who }}"',
                    args: { who: "default-a-bc" },
                    env: { vars: { MARK: "env-a-bc" } },
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
                    x: {
                        exec: 'echo "APP-X"',
                        dependencies: [n.abRef],
                        meta: { who: "app-x" },
                    },
                    y: {
                        exec: 'echo "APP-Y"',
                        dependencies: [n.aRef],
                        meta: { who: "app-y" },
                    },
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
                            cache: FLAT_CONFIG_HASH,
                        },
                    },
                },
            },
            files: { "app/a.txt": "a" },
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
        expect(item.hash).toBe("aZRuysvFOqM9KaWhJwamH3krAObxWkX7VmAHMSE3JUw=");
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

        // One marker per pair: the pair ran once, not once per result row.
        expect(
            output.split("RUN-AB-C mark=env-ab-c arg=default-ab-c"),
        ).toHaveLength(2);
        expect(
            output.split("RUN-A-BC mark=env-a-bc arg=default-a-bc"),
        ).toHaveLength(2);
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
        expect(output).toContain("RUN-AB-C mark=env-ab-c arg=default-ab-c");
        expect(output).toContain("RUN-A-BC mark=env-a-bc arg=default-a-bc");
    });

    test.each(["skip-dependents", "continue"])(
        "a failing pair does not fail the other pair or its consumer (-o %s)",
        async (onFailure) => {
            const ws = makeWorkspace({
                workspace: { projects: ["**"] },
                projects: {
                    "p-ab": {
                        name: names.abProject,
                        tasks: { [esc(names.abTask)]: { exec: "exit 3" } },
                    },
                    "p-a": {
                        name: names.aProject,
                        tasks: {
                            [esc(names.aTask)]: { exec: 'echo "A-BC-OK"' },
                        },
                    },
                    app: {
                        name: "app",
                        tasks: {
                            x: {
                                exec: 'echo "APP-X"',
                                dependencies: [names.abRef],
                            },
                            y: {
                                exec: 'echo "APP-Y"',
                                dependencies: [names.aRef],
                            },
                        },
                    },
                },
            });
            const resultPath = ws.path("result.json");
            const result = await runOmni(
                [
                    "run",
                    "x",
                    "y",
                    "-p",
                    "app",
                    "-o",
                    onFailure,
                    "--output-logs",
                    "all",
                    "--result",
                    resultPath,
                    "--result-format",
                    "json",
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveFailed();
            const items = JSON.parse(
                readFileSync(resultPath, "utf8"),
            ) as ResultItem[];
            const row = (p: string, t: string) => {
                const found = find(items, p, t);
                expect(found, `result row for ${p}#${t}`).toBeDefined();
                return found!;
            };
            const failed = row(names.abProject, names.abTask);
            expect(failed.status).toBe("errored");
            expect(row(names.aProject, names.aTask).status).toBe("completed");
            const skips = onFailure === "skip-dependents";
            expect(row("app", "x").status).toBe(
                skips ? "skipped" : "completed",
            );
            expect(row("app", "y").status).toBe("completed");
            expect(`${result.stdout}\n${result.stderr}`).toContain("APP-Y");
            expect(`${result.stdout}\n${result.stderr}`.includes("APP-X")).toBe(
                !skips,
            );
        },
    );

    test("cache prune --dir/--meta with one pair allowed touches only that pair", async () => {
        const ws = collisionWorkspace(names);
        await runApp(ws);

        for (const filter of [
            ["--dir", "p-ab"],
            ["--meta", 'who == "ab-c"'],
            ["--dir", "p-ab", "--meta", 'who == "ab-c"'],
        ]) {
            const dry = await runOmni(
                ["cache", "prune", ...filter, "--dry-run"],
                { cwd: ws.cwd },
            );
            expect(dry).toHaveSucceeded();
            const text = `${dry.stdout}\n${dry.stderr}`;
            expect(text).toContain(`Project: ${names.abProject}`);
            expect(text).not.toContain(`Project: ${names.aProject}\n`);
            expect(text).not.toContain(`Task: ${names.aTask}`);
        }

        const pruned = await runOmni(
            ["cache", "prune", "--meta", 'who == "ab-c"', "--yes"],
            { cwd: ws.cwd },
        );
        expect(pruned).toHaveSucceeded();

        const second = await runApp(ws);
        const hit = (p: string, t: string) =>
            find(second.items, p, t)?.cache_hit;
        expect(hit(names.abProject, names.abTask)).toBe(false);
        expect(hit(names.aProject, names.aTask)).toBe(true);
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

        expect(find(second.items, "app", "x")?.cache_hit).toBe(true);
        expect(find(second.items, "app", "y")?.cache_hit).toBe(true);
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

const B3_IV = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
    0x1f83d9ab, 0x5be0cd19,
];
const B3_PERM = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

/** BLAKE3 of an input of at most 64 bytes (a single chunk of one block). */
function blake3Small(input: Uint8Array): Buffer {
    if (input.length > 64) throw new Error("blake3Small: input over 64 bytes");
    const block = Buffer.alloc(64);
    block.set(input);
    let m: number[] = Array.from({ length: 16 }, (_, i) =>
        block.readUInt32LE(i * 4),
    );
    const v: number[] = [
        ...B3_IV,
        ...B3_IV.slice(0, 4),
        0,
        0,
        input.length,
        1 | 2 | 8,
    ];
    const at = (xs: number[], i: number) => xs[i] as number;
    const rotr = (x: number, n: number) => ((x >>> n) | (x << (32 - n))) >>> 0;
    const g = (
        a: number,
        b: number,
        c: number,
        d: number,
        x: number,
        y: number,
    ) => {
        v[a] = (at(v, a) + at(v, b) + at(m, x)) >>> 0;
        v[d] = rotr(at(v, d) ^ at(v, a), 16);
        v[c] = (at(v, c) + at(v, d)) >>> 0;
        v[b] = rotr(at(v, b) ^ at(v, c), 12);
        v[a] = (at(v, a) + at(v, b) + at(m, y)) >>> 0;
        v[d] = rotr(at(v, d) ^ at(v, a), 8);
        v[c] = (at(v, c) + at(v, d)) >>> 0;
        v[b] = rotr(at(v, b) ^ at(v, c), 7);
    };
    for (let round = 0; round < 7; round++) {
        g(0, 4, 8, 12, 0, 1);
        g(1, 5, 9, 13, 2, 3);
        g(2, 6, 10, 14, 4, 5);
        g(3, 7, 11, 15, 6, 7);
        g(0, 5, 10, 15, 8, 9);
        g(1, 6, 11, 12, 10, 11);
        g(2, 7, 8, 13, 12, 13);
        g(3, 4, 9, 14, 14, 15);
        m = B3_PERM.map((i) => at(m, i));
    }
    const out = Buffer.alloc(32);
    for (let i = 0; i < 8; i++)
        out.writeUInt32LE((at(v, i) ^ at(v, i + 8)) >>> 0, i * 4);
    return out;
}

/** `hash project` fold: `b3(b3(seed) || b3(leaf))` for one selected task. */
function projectHashOf(...leaves: Buffer[]): string {
    let acc = blake3Small(Buffer.from("DEFAULT_SEED"));
    for (const leaf of leaves)
        acc = blake3Small(Buffer.concat([acc, blake3Small(leaf)]));
    return base58(acc);
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
        expect(afterChange).toHaveSucceeded();
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
        expect(fresh).toHaveSucceeded();
        expect(`${fresh.stdout}\n${fresh.stderr}`).not.toContain("Project:");

        ws.write("p-ab/src/in.txt", "changed");
        const changed = await runOmni(["cache", "prune", "--stale", "-d"], {
            cwd: ws.cwd,
        });
        expect(changed).toHaveSucceeded();
        const text = `${changed.stdout}\n${changed.stderr}`;
        expect(text).toContain("Project: a#b");
        expect(text).toContain("  Task: c");
        expect(text).not.toContain("Task: b#c");
    });

    it("run and stale observation agree on a meta-templated cache path for each pair", async () => {
        const task = (dir: string, mark: string) => ({
            exec: `echo "${mark}"`,
            meta: { dir },
            cache: { key: { defaults: false, files: ["{{ meta.dir }}/**"] } },
        });
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "p-ab": {
                    name: "a#b",
                    tasks: { c: task("d-ab", "META-AB") },
                },
                "p-a": { name: "a", tasks: { "b\\#c": task("d-a", "META-A") } },
            },
            files: {
                "p-ab/d-ab/in.txt": "one",
                "p-ab/d-a/in.txt": "one",
                "p-a/d-a/in.txt": "one",
                "p-a/d-ab/in.txt": "one",
            },
        });
        const resultPath = ws.path("result.json");
        const run = await runOmni(
            [
                "run",
                "c",
                "b\\#c",
                "--result",
                resultPath,
                "--result-format",
                "json",
            ],
            { cwd: ws.cwd },
        );
        expect(run).toHaveSucceeded();
        const stale = async () => {
            const r = await runOmni(["cache", "prune", "--stale", "-d"], {
                cwd: ws.cwd,
            });
            expect(r).toHaveSucceeded();
            return `${r.stdout}\n${r.stderr}`;
        };
        expect(await stale()).not.toContain("Project:");

        // Only the templated directory of each pair is an input of that pair.
        ws.write("p-ab/d-a/in.txt", "unrelated");
        ws.write("p-a/d-ab/in.txt", "unrelated");
        expect(await stale()).not.toContain("Project:");

        ws.write("p-a/d-a/in.txt", "changed");
        const text = await stale();
        expect(text).toContain("Project: a\n");
        expect(text).not.toContain("Project: a#b");
    });

    it("cache prune prints the whole raw task name containing a hash", async () => {
        const ws = collisionWorkspace(COLLIDING);
        await runApp(ws);
        const dry = await runOmni(
            ["cache", "prune", "-d", "--project", "a", "--task", "b#c"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        const text = `${dry.stdout}\n${dry.stderr}`;
        expect(text).toContain("Project: a");
        expect(text).toContain("Task: b#c");
    });
});

describe("+hash @e2e (hash project uses the v1 leaf, formerly colliding pairs differ)", () => {
    it("matches an independent fold of the run digest and keeps the pairs apart", async () => {
        expect(blake3Small(Buffer.alloc(0)).toString("hex")).toBe(
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        );

        const task = () => ({
            exec: 'echo "same task"',
            env: { vars: { MARK: "same" } },
            args: { who: "same" },
            meta: { who: "same" },
            cache: NO_CONFIG_HASH,
        });
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "p-ab": { name: "a#b", tasks: { c: task() } },
                "p-a": { name: "a", tasks: { "b\\#c": task() } },
            },
            files: { "p-ab/src/in.txt": "one", "p-a/src/in.txt": "one" },
        });
        const resultPath = ws.path("result.json");
        const run = await runOmni(
            [
                "run",
                "c",
                "b\\#c",
                "--result",
                resultPath,
                "--result-format",
                "json",
            ],
            { cwd: ws.cwd },
        );
        expect(run).toHaveSucceeded();
        const items = JSON.parse(
            readFileSync(resultPath, "utf8"),
        ) as ResultItem[];
        const leaf = (project: string, task: string) =>
            Buffer.from(find(items, project, task)?.hash as string, "base64");
        expect(leaf("a#b", "c").equals(leaf("a", "b#c"))).toBe(false);

        const hashProject = async (project: string, task: string) => {
            const r = await runOmni(
                ["hash", "-r", "project", project, "-t", esc(task)],
                { cwd: ws.cwd },
            );
            expect(r).toHaveSucceeded();
            return r.stdout.trim();
        };
        const first = await hashProject("a#b", "c");
        const second = await hashProject("a", "b#c");
        expect(first).not.toBe(second);
        expect(first).toBe(projectHashOf(leaf("a#b", "c")));
        expect(second).toBe(projectHashOf(leaf("a", "b#c")));

        ws.write("p-ab/src/in.txt", "changed");
        expect(await hashProject("a#b", "c")).not.toBe(first);
        expect(await hashProject("a", "b#c")).toBe(second);
    });
});

describe("+hash @e2e (hash project folds the selected leaves for every filter shape)", () => {
    it("matches an independent fold for no filter, exact and exact-plus-glob", async () => {
        const cache = { key: { defaults: false, files: ["src/**"] } };
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                lib: {
                    name: "lib",
                    tasks: { gen: { exec: "echo gen", cache } },
                },
                app: {
                    name: "app",
                    tasks: {
                        x: {
                            exec: "echo x",
                            cache,
                            dependencies: ["lib#gen"],
                        },
                        xa: { exec: "echo xa", cache },
                        y: { exec: "echo y", cache },
                    },
                },
            },
            files: { "lib/src/a.txt": "a", "app/src/b.txt": "b" },
        });
        const items = await runResult(ws, ["x", "xa", "y", "-p", "app"]);
        const leaf = (task: string) =>
            Buffer.from(find(items, "app", task)?.hash as string, "base64");
        const hashProject = async (...filter: string[]) => {
            const r = await runOmni(
                ["hash", "-r", "project", "app", ...filter],
                {
                    cwd: ws.cwd,
                },
            );
            expect(r).toHaveSucceeded();
            return r.stdout.trim();
        };

        expect(await hashProject()).toBe(
            projectHashOf(leaf("x"), leaf("xa"), leaf("y")),
        );
        expect(await hashProject("-t", "x")).toBe(projectHashOf(leaf("x")));
        // The dependency-only `lib#gen` node feeds x's leaf but is not folded.
        expect(await hashProject("-t", "y", "--task-pattern", "x*")).toBe(
            projectHashOf(leaf("x"), leaf("xa"), leaf("y")),
        );
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

describe("+run @e2e (an entry stored at a different address is never reused)", () => {
    it("misses, rebuilds under the v1 address and leaves the entry at the other address untouched", async () => {
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
        // Different-address check only: the entry is a v1 entry moved to an
        // arbitrary address, not an entry written by the base binary. True v0
        // non-reuse against a base-produced entry stays a recorded gap (see
        // the F03.4 slice 0 plan); no base binary is available to this harness.
        const foreign = "LegacyKeyDigestFor";
        renameSync(join(projectDir, digest), join(projectDir, foreign));

        const second = await runResult(ws, ["x", "-p", "app"]);
        expect(second[0]?.cache_hit).toBe(false);
        expect(second[0]?.hash).toBe(first[0]?.hash);
        expect(readdirSync(projectDir).sort()).toEqual(
            [digest, foreign].sort(),
        );
    });
});
