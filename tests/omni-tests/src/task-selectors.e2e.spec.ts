/**
 * RFC 0024 R06/F04: the planner takes explicit exact/pattern/all task
 * selectors. Positional CLI inputs are exact names and `--task-pattern`
 * selects by glob. The internal "all tasks" callers (cache stats/prune/
 * staleness, project hashing) must keep selecting every task, including a task
 * literally named `*`.
 */

import { rmSync } from "node:fs";

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

    it("`prune --stale` plans all tasks and keeps fresh entries", async () => {
        const ws = makeWorkspace(spec());
        expect(
            await runOmni(["run", "--task-pattern", "*"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const before = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(before).toOutputContaining("- Task: *");

        const dry = await runOmni(["cache", "prune", "--stale", "--dry-run"], {
            cwd: ws.cwd,
        });

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

function multiSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "svc/app": { name: "app", tasks: { build: 'echo "build-app"' } },
            "svc/app2": { name: "app2", tasks: { build: 'echo "build-app2"' } },
            "svc/lib": { name: "lib", tasks: { build: 'echo "build-lib"' } },
        },
    };
}

describe("+run @e2e (project filters are exact, --project-pattern is a glob)", () => {
    it("`-p app` runs only the project named `app`", async () => {
        const ws = makeWorkspace(multiSpec());

        const result = await runOmni(
            ["run", "build", "-p", "app", "--output-logs", "all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result.stderr).not.toContain("app2#build");
        expect(result.stderr).not.toContain("lib#build");
    });

    it("`-p 'app*'` fails and points at --project-pattern", async () => {
        const ws = makeWorkspace(multiSpec());

        const result = await runOmni(["run", "build", "-p", "app*"], {
            cwd: ws.cwd,
        });

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no project found for filter: app*");
        expect(result.stderr).toContain("--project-pattern");
    });

    it("`--project-pattern 'app*'` matches by glob", async () => {
        const ws = makeWorkspace(multiSpec());

        const result = await runOmni(
            [
                "run",
                "build",
                "--project-pattern",
                "app*",
                "--output-logs",
                "all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("app#build");
        expect(result).toHaveStderrContaining("app2#build");
        expect(result.stderr).not.toContain("lib#build");
    });

    it("exact and pattern project selectors combine", async () => {
        const ws = makeWorkspace(multiSpec());

        const result = await runOmni(
            [
                "run",
                "build",
                "-p",
                "lib",
                "--project-pattern",
                "app?",
                "--output-logs",
                "all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("lib#build");
        expect(result).toHaveStderrContaining("app2#build");
        expect(result.stderr).not.toContain("app#build");
    });

    it("an unknown exact project fails even beside a known one", async () => {
        const ws = makeWorkspace(multiSpec());

        const result = await runOmni(
            ["run", "build", "-p", "app", "-p", "nope"],
            { cwd: ws.cwd },
        );

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("no project found for filter: nope");
    });

    it("`exec -p` is exact and `exec --project-pattern` is a glob", async () => {
        const ws = makeWorkspace(multiSpec());

        const exact = await runOmni(["exec", "-p", "app", "--", "echo", "hi"], {
            cwd: ws.cwd,
        });
        expect(exact).toHaveSucceeded();

        const glob = await runOmni(["exec", "-p", "app*", "--", "echo", "hi"], {
            cwd: ws.cwd,
        });
        expect(glob).not.toHaveSucceeded();
        expect(glob.stderr).toContain("--project-pattern");
    });
});

describe("+cache @cache (project selectors on stats and prune)", () => {
    const warm = async (cwd: string) => {
        expect(
            await runOmni(["run", "build", "--project-pattern", "*"], { cwd }),
        ).toHaveSucceeded();
    };

    it("`stats --project app` is exact; `--project-pattern 'app*'` also matches app2", async () => {
        const ws = makeWorkspace(multiSpec());
        await warm(ws.cwd);

        const exact = await runOmni(["cache", "stats", "--project", "app"], {
            cwd: ws.cwd,
        });
        expect(exact).toHaveSucceeded();
        expect(exact).toOutputContaining("Project: app");
        expect(exact.out).not.toContain("Project: app2");
        expect(exact.out).not.toContain("Project: lib");

        const glob = await runOmni(
            ["cache", "stats", "--project-pattern", "app*"],
            { cwd: ws.cwd },
        );
        expect(glob).toHaveSucceeded();
        expect(glob).toOutputContaining("Project: app2");
        expect(glob.out).not.toContain("Project: lib");
    });

    it("`prune --project 'app*'` deletes nothing; an exact name prunes only that project", async () => {
        const ws = makeWorkspace(multiSpec());
        await warm(ws.cwd);

        const glob = await runOmni(
            ["cache", "prune", "--project", "app*", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(glob).toHaveSucceeded();
        expect(glob).toOutputContaining(
            "No cache entries matched the given filters",
        );

        const exact = await runOmni(
            ["cache", "prune", "--project", "lib", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(exact).toHaveSucceeded();
        expect(exact).toOutputContaining("lib");
        expect(exact.out).not.toContain("app");
    });

    it("an exact project that no longer exists is not an error, even with --dir", async () => {
        const ws = makeWorkspace(multiSpec());
        await warm(ws.cwd);

        const result = await runOmni(
            [
                "cache",
                "prune",
                "--project",
                "gone",
                "--dir",
                "svc/**",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("entries of a removed project are orphaned, not stale", async () => {
        const ws = makeWorkspace(multiSpec());
        await warm(ws.cwd);
        // The project disappears from the workspace; its cache entries stay.
        rmSync(ws.path("svc/lib"), { recursive: true, force: true });

        const exact = await runOmni(
            ["cache", "prune", "--project", "lib", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(exact).toHaveSucceeded();
        expect(exact).toOutputContaining("lib");

        const stale = await runOmni(
            ["cache", "prune", "--project", "lib", "--stale", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(stale).toHaveSucceeded();
        expect(stale).toOutputContaining(
            "No cache entries matched the given filters",
        );

        const orphan = await runOmni(
            ["cache", "prune", "--project", "lib", "--orphan", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(orphan).toHaveSucceeded();
        expect(orphan).toOutputContaining("Project: lib");

        // --dir matches only projects present in the workspace.
        const byDir = await runOmni(
            [
                "cache",
                "prune",
                "--project",
                "lib",
                "--dir",
                "svc/**",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );
        expect(byDir).toHaveSucceeded();
        expect(byDir).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });
});
