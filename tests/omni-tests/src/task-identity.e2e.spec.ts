/* oxlint-disable vitest/no-standalone-expect -- `pendingCutover` is an alias of `it`/`it.fails` */
/**
 * RFC 0024 F03.4 slice 0 proof: a canonical task is its raw (project, task)
 * pair. Project `a#b` with task `c` and project `a` with task `b#c` render the
 * same unescaped `a#b#c` label, which the base still uses as an identity in
 * context lookups, result maps, output routing and the cache digest.
 *
 * The first block pins the base behavior of an ordinary undecorated task (it
 * must pass before and after the cutover, except for the digest, which moves
 * with the v1 identity leaf). The second block is the collision fixture: it
 * documents the target behavior and is `it.fails` until the coherent cutover
 * lands, so the suite stays green while still proving, on the base, that the
 * assertions fail for the collision (see the failure notes per test). Flip
 * `pendingCutover` to `it` in the cutover commit.
 */

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type Workspace } from "@/harness";

// Set OMNI_IDENTITY_STRICT=1 to run the collision tests as ordinary tests and
// see the base failures.
const pendingCutover = process.env.OMNI_IDENTITY_STRICT ? it : it.fails;

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

    it("keeps its digest for controlled inputs (moves with the v1 leaf)", async () => {
        const ws = ordinary();
        const resultPath = ws.path("result.json");
        await runOmni(
            ["run", "build", "--result", resultPath, "--result-format", "json"],
            { cwd: ws.cwd },
        );
        const [item] = JSON.parse(readFileSync(resultPath, "utf8"));
        expect(item.hash).toBe("QJKrdUfv3oyXl5FNHDNXeTg2VEfwDfPPItr1Hw8AlFo=");
    });
});

function behaviors(names: Names, test: typeof it | typeof it.fails): void {
    const find = (items: ResultItem[], project: string, task: string) =>
        items.find(
            (i) => i.task.project_name === project && i.task.task_name === task,
        );

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
    behaviors(COLLIDING, pendingCutover);
});

describe("+run @e2e (same-project tasks keep their own cached logs)", () => {
    // Base defect found by the proof, independent of label collisions:
    // `cache_many` re-associates logs through a project-only map, so a
    // cache hit replays one task's logs for its sibling.
    pendingCutover("replays each task's own logs on a cache hit", async () => {
        const ws = collisionWorkspace(CONTROL);
        await runApp(ws);
        const second = await runApp(ws, ["--output-cached-logs", "all"]);
        const output = `${second.stdout}\n${second.stderr}`;

        expect(output).toContain("APP-X");
        expect(output).toContain("APP-Y");
    });
});
