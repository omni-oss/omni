/**
 * RFC 0024 R06/F04: the planner takes explicit exact/pattern/all task
 * selectors. Positional CLI inputs are exact names and `--task-pattern`
 * selects by glob. The internal "all tasks" callers (cache stats/prune/
 * staleness, project hashing) must keep selecting every task, including a task
 * literally named `*`.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function spec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "svc/app": {
                name: "app",
                tasks: {
                    "*": 'echo "star"',
                    build: 'echo "b"',
                    test: 'echo "t"',
                },
            },
        },
    };
}

describe("+run @e2e (positional tasks are exact, --task-pattern is a glob)", () => {
    it("`run '*'` runs only the task literally named `*`", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "*", "--output-logs", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#*");
        expect(result.stderr).not.toContain("app#build");
        expect(result.stderr).not.toContain("app#test");
    });

    it("`--task-pattern '*'` expands to every task", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(
            ["run", "--task-pattern", "*", "--output-logs", "all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result).toHaveStderrContaining("app#test");
        expect(result).toHaveStderrContaining("app#*");
    });

    it("`--task-pattern 'bu*'` matches by glob", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "--task-pattern", "bu*"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result.stderr).not.toContain("app#test");
    });

    it("exact and pattern selectors combine", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(
            ["run", "build", "--task-pattern", "te*"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result).toHaveStderrContaining("app#test");
        expect(result.stderr).not.toContain("app#*");
    });

    it("`run 'bu*'` fails and points at --task-pattern", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "bu*"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `bu*`");
        expect(result.stderr).toContain("--task-pattern");
    });

    it("an unknown exact name fails", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "nope"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `nope`");
    });
});

describe("+cache @cache (internal all-tasks selection)", () => {
    it("plan-based `cache stats --dir` with no -t lists every cached task", async () => {
        const ws = makeWorkspace(spec());
        expect(
            await runOmni(["run", "--task-pattern", "*"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const stats = await runOmni(["cache", "stats", "--dir", "svc/**"], {
            cwd: ws.cwd,
        });

        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("- Task: *");
        expect(stats).toOutputContaining("- Task: build");
        expect(stats).toOutputContaining("- Task: test");
    });

    it("`prune --stale-only` plans all tasks and keeps fresh entries", async () => {
        const ws = makeWorkspace(spec());
        expect(
            await runOmni(["run", "--task-pattern", "*"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const before = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(before).toOutputContaining("- Task: *");

        const dry = await runOmni(
            ["cache", "prune", "--stale-only", "--dry-run"],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );

        const after = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(after).toOutputContaining("- Task: *");
    });
});

describe("+hash @e2e (project hash selectors)", () => {
    const hashProject = (cwd: string, ...extra: string[]) =>
        runOmni(["hash", "-r", "project", "app", ...extra], { cwd });

    it("no selector and `--task-pattern '*'` hash every task alike", async () => {
        const ws = makeWorkspace(spec());

        const all = await hashProject(ws.cwd);
        const star = await hashProject(ws.cwd, "--task-pattern", "*");

        expect(all).toHaveSucceeded();
        expect(star).toHaveSucceeded();
        expect(all.stdout).toBe(star.stdout);
    });

    it("`-t '*'` is exact: it hashes only the task named `*`", async () => {
        const ws = makeWorkspace(spec());

        const all = await hashProject(ws.cwd);
        const exactStar = await hashProject(ws.cwd, "-t", "*");

        expect(exactStar).toHaveSucceeded();
        expect(exactStar.stdout).not.toBe(all.stdout);
    });

    it("`-t build` and `--task-pattern build` select the same task", async () => {
        const ws = makeWorkspace(spec());

        const exact = await hashProject(ws.cwd, "-t", "build");
        const pattern = await hashProject(ws.cwd, "--task-pattern", "build");
        const all = await hashProject(ws.cwd);

        expect(exact).toHaveSucceeded();
        expect(exact.stdout).toBe(pattern.stdout);
        expect(exact.stdout).not.toBe(all.stdout);
    });

    it("a glob passed to `-t` fails and points at --task-pattern", async () => {
        const ws = makeWorkspace(spec());

        const result = await hashProject(ws.cwd, "-t", "bu*");

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `bu*`");
        expect(result.stderr).toContain("--task-pattern");
    });

    it("an unknown exact task name fails", async () => {
        const ws = makeWorkspace(spec());

        const result = await hashProject(ws.cwd, "-t", "nope");

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no task named `nope`");
    });
});

describe("+cache @cache (task selectors on stats and prune)", () => {
    const warm = async (cwd: string) => {
        expect(
            await runOmni(["run", "--task-pattern", "*"], { cwd }),
        ).toHaveSucceeded();
    };

    it("`stats -t build*` is exact and matches nothing", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        const stats = await runOmni(["cache", "stats", "-t", "build*"], {
            cwd: ws.cwd,
        });

        expect(stats).toHaveSucceeded();
        expect(stats.out).not.toContain("- Task: build");
    });

    it("`stats -t '*'` is exact and lists only the task named `*`", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        const stats = await runOmni(["cache", "stats", "-t", "*"], {
            cwd: ws.cwd,
        });

        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("- Task: *");
        expect(stats.out).not.toContain("- Task: build");
        expect(stats.out).not.toContain("- Task: test");
    });

    it("`stats --task-pattern 'bu*'` matches by glob", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        const stats = await runOmni(
            ["cache", "stats", "--task-pattern", "bu*"],
            { cwd: ws.cwd },
        );

        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("- Task: build");
        expect(stats.out).not.toContain("- Task: test");
    });

    it("`prune -t build*` deletes nothing; an exact name prunes only that task", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        const glob = await runOmni(
            ["cache", "prune", "-t", "build*", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(glob).toHaveSucceeded();
        expect(glob).toOutputContaining(
            "No cache entries matched the given filters",
        );

        const exact = await runOmni(
            ["cache", "prune", "-t", "build", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(exact).toHaveSucceeded();
        expect(exact).toOutputContaining("build");
        expect(exact.out).not.toContain("test");
    });

    it("an exact name for a task that no longer exists is not an error", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        // Plan-based filters (--dir) must tolerate a name with no live task.
        const result = await runOmni(
            ["cache", "prune", "-t", "gone", "--dir", "svc/**", "--dry-run"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("plan-based filters keep exact and pattern selectors apart", async () => {
        const ws = makeWorkspace(spec());
        await warm(ws.cwd);

        const exact = await runOmni(
            ["cache", "prune", "-t", "bu*", "--dir", "svc/**", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(exact).toHaveSucceeded();
        expect(exact).toOutputContaining(
            "No cache entries matched the given filters",
        );

        const pattern = await runOmni(
            [
                "cache",
                "prune",
                "--task-pattern",
                "bu*",
                "--dir",
                "svc/**",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );
        expect(pattern).toHaveSucceeded();
        expect(pattern).toOutputContaining("build");
        expect(pattern.out).not.toContain("test");
    });
});
