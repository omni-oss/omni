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

describe("+hash @e2e (project hash with no -t covers all tasks)", () => {
    it("matches the hash from the glob `-t '*'`", async () => {
        const ws = makeWorkspace(spec());

        const all = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        const star = await runOmni(
            ["hash", "-r", "project", "app", "-t", "*"],
            { cwd: ws.cwd },
        );

        expect(all).toHaveSucceeded();
        expect(star).toHaveSucceeded();
        expect(all.stdout).toBe(star.stdout);
    });

    it("differs from a single-task hash", async () => {
        const ws = makeWorkspace(spec());

        const all = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        const build = await runOmni(
            ["hash", "-r", "project", "app", "-t", "build"],
            { cwd: ws.cwd },
        );

        expect(all.stdout).not.toBe(build.stdout);
    });
});
