/**
 * RFC 0024 R06 parity: the planner now takes explicit exact/pattern/all task
 * selectors. Positional CLI inputs are still globs, and the internal
 * "all tasks" callers (cache stats/prune/staleness, project hashing) must keep
 * selecting every task, including a task literally named `*`.
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

describe("+run @e2e (positional tasks stay globs)", () => {
    it("`run '*'` still expands to every task", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "*", "--output-logs", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result).toHaveStderrContaining("app#test");
        expect(result).toHaveStderrContaining("app#*");
    });

    it("`run 'bu*'` still matches by glob", async () => {
        const ws = makeWorkspace(spec());

        const result = await runOmni(["run", "bu*"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result.stderr).not.toContain("app#test");
    });
});

describe("+cache @cache (internal all-tasks selection)", () => {
    it("plan-based `cache stats --dir` with no -t lists every cached task", async () => {
        const ws = makeWorkspace(spec());
        expect(
            await runOmni(["run", "build", "test"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const stats = await runOmni(["cache", "stats", "--dir", "svc/**"], {
            cwd: ws.cwd,
        });

        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("build");
        expect(stats).toOutputContaining("test");
    });

    it("`prune --stale-only` plans all tasks and keeps fresh entries", async () => {
        const ws = makeWorkspace(spec());
        expect(
            await runOmni(["run", "build", "test"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const dry = await runOmni(
            ["cache", "prune", "--stale-only", "--dry-run"],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
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
