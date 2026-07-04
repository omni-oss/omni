/**
 * `omni cache` e2e tests: the cache directory, run-time cache hit/persistence
 * behavior, `cache stats`, `cache prune` (incl. interactive confirmation and
 * filters), and `cache remote setup`.
 *
 * The task cache lives at `<workspace root>/.omni/cache`, so every test runs in
 * a fresh temp workspace and the cache is naturally isolated. Tasks here use
 * `echo`, which omni caches by default. Cached output is replayed only when the
 * `cached` output-logs policy asks for it; the default (`failed`) stays quiet on
 * successful cache hits, and `--output-cached-logs all` forces replay.
 */

import { rmSync } from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, it } from "vitest";
import {
    makeWorkspace,
    runOmni,
    singleProjectSpec,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";
import { cleanPath } from "./utils";

/** A workspace with `alpha` (build+test) and `beta` (build) echo tasks. */
function multiTaskSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            alpha: {
                name: "alpha",
                tasks: {
                    build: 'echo "build alpha"',
                    test: 'echo "test alpha"',
                },
            },
            beta: { name: "beta", tasks: { build: 'echo "build beta"' } },
        },
    };
}

/** Run tasks once so their results land in the workspace cache. */
async function seedCache(ws: Workspace, tasks: string[]): Promise<void> {
    const result = await runOmni(["run", ...tasks], { cwd: ws.cwd });
    expect(result).toHaveSucceeded();
}

/** A workspace with projects living in distinct directories. */
function multiDirSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "svc/api": { name: "api", tasks: { build: 'echo "build api"' } },
            other: { name: "other", tasks: { build: 'echo "build other"' } },
        },
    };
}

/** A workspace with two tasks tagged with distinct `meta.tier` values. */
function metaTieredSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: {
                    fast: { exec: 'echo "fast"', meta: { tier: "fast" } },
                    slow: { exec: 'echo "slow"', meta: { tier: "slow" } },
                },
            },
        },
    };
}

describe("+cache @cache (cache dir)", () => {
    it("`cache dir` prints the workspace cache directory", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["cache", "dir"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(cleanPath(result.out)).toBe(ws.path(".omni", "cache"));
    });
});

describe("+cache @cache @e2e (run cache hit / log replay)", () => {
    it("`--output-cached-logs all` replays a cache hit's logs", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();
        expect(first.stdout).not.toContain("Cache hits");

        const second = await runOmni(
            ["run", "build", "--output-cached-logs", "all"],
            { cwd: ws.cwd },
        );
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hit for task 'app#build'");
        expect(second).toOutputContaining("(replaying logs)");
        // The recorded stdout is replayed.
        expect(second).toOutputContaining("build app");
        expect(second).toOutputContaining("Cache hits");
    });

    it("a successful cache hit stays quiet by default (failed policy)", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const replayed = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(replayed).toHaveSucceeded();
        expect(replayed).toOutputContaining("Cache hit for task 'app#build'");
        expect(replayed).toOutputContaining("(skipping logs)");
        // With the default `failed` policy, a successful cache hit is not replayed.
        expect(replayed.stdout).not.toContain("build app");
    });

    it("`--output-cached-logs never` reports the hit but suppresses replayed logs", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const replayed = await runOmni(
            ["run", "build", "--output-cached-logs", "never"],
            { cwd: ws.cwd },
        );
        expect(replayed).toHaveSucceeded();
        expect(replayed).toOutputContaining("Cache hit for task 'app#build'");
        expect(replayed).toOutputContaining("(skipping logs)");
        // With logs skipped, the cached task's stdout is not replayed.
        expect(replayed.stdout).not.toContain("build app");
    });
});

describe("+cache @cache (run cache persistence)", () => {
    it("`--no-cache` does not persist results to the cache", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const run = await runOmni(["run", "build", "--no-cache"], {
            cwd: ws.cwd,
        });
        expect(run).toHaveSucceeded();
        expect(run.stdout).not.toContain("Cache hits");

        // Nothing was written to the cache, so stats reports no tasks.
        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("(No tasks)");
        expect(stats.stdout).not.toContain("- Task: build");
    });

    it("`-f/--force` re-executes a task even when it is cached", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const forced = await runOmni(["run", "build", "--force"], {
            cwd: ws.cwd,
        });
        expect(forced).toHaveSucceeded();
        // Forced execution re-runs the task instead of replaying the cache.
        expect(forced).toOutputContaining("Executed task 'app#build'");
        expect(forced.stdout).not.toContain("Cache hits");
        expect(forced.stdout).not.toContain("Cache hit for task");
    });
});

describe("+cache @cache (stats)", () => {
    it("`cache stats` lists cached projects/tasks with sizes and timestamps", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("Project: app");
        expect(stats).toOutputContaining("- Task: build");
        expect(stats).toOutputContaining("Created:");
        expect(stats).toOutputContaining("File Sizes:");
    });

    it("`cache stats -p/-t` filters projects and tasks by glob", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        const byProject = await runOmni(["cache", "stats", "-p", "alpha"], {
            cwd: ws.cwd,
        });
        expect(byProject).toHaveSucceeded();
        expect(byProject).toOutputContaining("Project: alpha");
        expect(byProject.stdout).not.toContain("Project: beta");

        const byTask = await runOmni(["cache", "stats", "-t", "build"], {
            cwd: ws.cwd,
        });
        expect(byTask).toHaveSucceeded();
        expect(byTask).toOutputContaining("- Task: build");
        expect(byTask.stdout).not.toContain("- Task: test");
    });

    it("`cache stats --dir` filters by the owning project's directory", async () => {
        const ws = makeWorkspace(multiDirSpec());

        await seedCache(ws, ["build"]);

        const stats = await runOmni(["cache", "stats", "--dir", "svc/**"], {
            cwd: ws.cwd,
        });
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("Project: api");
        expect(stats.stdout).not.toContain("Project: other");
    });

    it("`cache stats -m/--meta` filters by task meta (CEL)", async () => {
        const ws = makeWorkspace(metaTieredSpec());

        await seedCache(ws, ["fast", "slow"]);

        const stats = await runOmni(
            ["cache", "stats", "-m", 'tier == "fast"'],
            { cwd: ws.cwd },
        );
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("- Task: fast");
        expect(stats.stdout).not.toContain("- Task: slow");
    });
});

describe("+cache @cache (prune)", () => {
    it("`prune --dry-run` lists matching entries but deletes nothing", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const dry = await runOmni(["cache", "prune", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: app");
        expect(dry).toOutputContaining("Dry mode enabled, would prune");

        // The entry survives a dry run.
        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toOutputContaining("- Task: build");
    });

    it("`prune -y/--yes` prunes without a confirmation prompt", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const pruned = await runOmni(["cache", "prune", "--yes"], {
            cwd: ws.cwd,
        });
        expect(pruned).toHaveSucceeded();
        expect(pruned).toOutputContaining("--- Cache Entries ---");
        expect(pruned).toOutputContaining("Pruned 1 cache entries");
        expect(pruned.stdout).not.toContain("Are you sure");

        // The cache is now empty.
        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toOutputContaining("(No tasks)");
    });

    it("`prune` with no matching entries warns and exits cleanly", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const dry = await runOmni(["cache", "prune", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });
});

describe("+cache @input (prune interactive confirmation)", () => {
    it("`n` at the confirmation prompt aborts and keeps the cache", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const aborted = await runOmni(["cache", "prune"], {
            cwd: ws.cwd,
            input: "n\n",
        });
        expect(aborted).toHaveSucceeded();
        expect(aborted).toOutputContaining(
            "Are you sure you want to prune the cache",
        );
        expect(aborted).toOutputContaining("Aborting");

        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toOutputContaining("- Task: build");
    });

    it("`y` at the confirmation prompt proceeds and prunes the cache", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        const confirmed = await runOmni(["cache", "prune"], {
            cwd: ws.cwd,
            input: "y\n",
        });
        expect(confirmed).toHaveSucceeded();
        expect(confirmed).toOutputContaining("Proceeding to prune the cache");
        expect(confirmed).toOutputContaining("Pruned 1 cache entries");

        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toOutputContaining("(No tasks)");
    });
});

describe("+cache @cache (prune filters)", () => {
    it("`--project` narrows pruning to matching projects", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        const dry = await runOmni(
            ["cache", "prune", "--project", "alpha", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: alpha");
        expect(dry.stdout).not.toContain("Project: beta");
    });

    it("`--task` narrows pruning to matching tasks", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        const dry = await runOmni(
            ["cache", "prune", "--task", "test", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Task: test");
        expect(dry.stdout).not.toContain("Task: build");
    });

    it("`--dir` narrows pruning to projects under matching directories", async () => {
        const ws = makeWorkspace(multiDirSpec());

        await seedCache(ws, ["build"]);

        const dry = await runOmni(
            ["cache", "prune", "--dir", "svc/**", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: api");
        expect(dry.stdout).not.toContain("Project: other");
    });

    it("`--meta` narrows pruning to tasks matching the CEL expression", async () => {
        const ws = makeWorkspace(metaTieredSpec());

        await seedCache(ws, ["fast", "slow"]);

        const dry = await runOmni(
            ["cache", "prune", "--meta", 'tier == "fast"', "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Task: fast");
        expect(dry.stdout).not.toContain("Task: slow");
    });

    it("`--older-than` excludes entries newer than the cutoff", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        // The just-created entry is younger than 1h, so nothing matches.
        const dry = await runOmni(
            ["cache", "prune", "--older-than", "1h", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("`--larger-than` excludes entries smaller than the threshold", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        // The echo task's cache entry is well under 1GB.
        const dry = await runOmni(
            ["cache", "prune", "--larger-than", "1GB", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("`--stale-only` keeps fresh (unchanged) entries", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        // Inputs are unchanged, so the cached entry is fresh and not pruned.
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

describe("+cache @cache (combined filters)", () => {
    /** Projects across dirs, tasks tagged with meta, so each filter can narrow. */
    function dirMetaSpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                "svc/api": {
                    name: "api",
                    tasks: {
                        build: {
                            exec: 'echo "api build"',
                            meta: { tier: "fast" },
                        },
                        test: {
                            exec: 'echo "api test"',
                            meta: { tier: "slow" },
                        },
                    },
                },
                "svc/web": {
                    name: "web",
                    tasks: {
                        build: {
                            exec: 'echo "web build"',
                            meta: { tier: "slow" },
                        },
                    },
                },
                other: {
                    name: "other",
                    tasks: {
                        build: {
                            exec: 'echo "other build"',
                            meta: { tier: "fast" },
                        },
                    },
                },
            },
        };
    }

    it("`stats -p -t --dir -m` narrows to the intersection of all four filters", async () => {
        const ws = makeWorkspace(dirMetaSpec());

        await seedCache(ws, ["build", "test"]);

        // Only `api#build` matches every filter: project `api`, task `build`,
        // dir `svc/**`, and meta `tier == fast`.
        const stats = await runOmni(
            [
                "cache",
                "stats",
                "-p",
                "api",
                "-t",
                "build",
                "--dir",
                "svc/**",
                "-m",
                'tier == "fast"',
            ],
            { cwd: ws.cwd },
        );

        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("Project: api");
        expect(stats).toOutputContaining("- Task: build");
        // Everything narrowed out by at least one filter is absent.
        expect(stats.stdout).not.toContain("Project: web");
        expect(stats.stdout).not.toContain("Project: other");
        expect(stats.stdout).not.toContain("- Task: test");
    });

    it("`prune --project --task` narrows to the intersection (dry-run)", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        const dry = await runOmni(
            [
                "cache",
                "prune",
                "--project",
                "alpha",
                "--task",
                "build",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: alpha");
        expect(dry).toOutputContaining("Task: build");
        // `beta` (other project) and `alpha#test` (other task) are excluded.
        expect(dry.stdout).not.toContain("Project: beta");
        expect(dry.stdout).not.toContain("Task: test");
    });

    it("`prune --dir --meta` combines the two context-backed filters (dry-run)", async () => {
        const ws = makeWorkspace(dirMetaSpec());

        await seedCache(ws, ["build", "test"]);

        // `svc/**` keeps api + web; `tier == fast` keeps api + other; their
        // intersection is only `api#build`.
        const dry = await runOmni(
            [
                "cache",
                "prune",
                "--dir",
                "svc/**",
                "--meta",
                'tier == "fast"',
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: api");
        expect(dry.stdout).not.toContain("Project: web");
        expect(dry.stdout).not.toContain("Project: other");
    });

    it("`prune --stale-only --older-than` applies both staleness gates", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        await seedCache(ws, ["build"]);

        // Entry is fresh and younger than 1h, so neither gate matches it.
        const dry = await runOmni(
            [
                "cache",
                "prune",
                "--stale-only",
                "--older-than",
                "1h",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("`prune --larger-than --project` combines a size gate with a project filter (dry-run)", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        // `alpha` matches the project filter, but no entry is over 1GB.
        const dry = await runOmni(
            [
                "cache",
                "prune",
                "--larger-than",
                "1GB",
                "--project",
                "alpha",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("`prune --dry-run` with matching filters never deletes", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        await seedCache(ws, ["build", "test"]);

        const dry = await runOmni(
            ["cache", "prune", "--project", "alpha", "--dry-run"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry).toOutputContaining("Project: alpha");

        // Despite matching entries, dry-run leaves the cache intact.
        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("Project: alpha");
        expect(stats).toOutputContaining("- Task: build");
    });
});

describe("+cache @cache (no-cache + stats)", () => {
    it("`run --no-cache` leaves the task out of `cache stats`", async () => {
        const ws = makeWorkspace(multiTaskSpec());

        // `build` is cached normally; `test` runs with --no-cache.
        await seedCache(ws, ["build"]);
        const uncached = await runOmni(
            ["run", "test", "-p", "alpha", "--no-cache"],
            { cwd: ws.cwd },
        );
        expect(uncached).toHaveSucceeded();

        const stats = await runOmni(["cache", "stats"], { cwd: ws.cwd });
        expect(stats).toHaveSucceeded();
        expect(stats).toOutputContaining("- Task: build");
        expect(stats.stdout).not.toContain("- Task: test");
    });
});

describe("+cache @cli (prune arg conflicts)", () => {
    it("`--dry-run` conflicts with `--yes`", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["cache", "prune", "--dry-run", "--yes"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining("cannot be used with '--yes'");
    });
});

describe("+cache @cache (remote setup)", () => {
    let server: http.Server | undefined;

    afterEach(async () => {
        if (server) {
            await new Promise<void>((resolve) =>
                server?.close(() => resolve()),
            );
            server = undefined;
        }
    });

    /**
     * Start a throwaway server that mimics a remote cache: `validate_access`
     * issues a HEAD to `/v1/artifacts` and treats any 2xx as valid access.
     */
    async function startMockRemoteCache(): Promise<string> {
        server = http.createServer((_req, res) => {
            res.statusCode = 200;
            res.end();
        });
        await new Promise<void>((resolve) =>
            server?.listen(0, "127.0.0.1", resolve),
        );
        const { port } = server.address() as AddressInfo;
        return `http://127.0.0.1:${port}`;
    }

    it("writes a yaml config when access validates", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            // `.omni` must exist for the config file's parent dir to be present.
            files: { ".omni/.keep": "" },
        });
        const baseUrl = await startMockRemoteCache();

        const result = await runOmni(
            [
                "cache",
                "remote",
                "setup",
                "-b",
                baseUrl,
                "-a",
                "test-key",
                "-t",
                "test-tenant",
                "-o",
                "test-org",
                "-w",
                "test-ws",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists(".omni/remote-cache.omni.yaml")).toBe(true);
        const config = ws.read(".omni/remote-cache.omni.yaml");
        expect(config).toContain(`api_base_url: "${baseUrl}"`);
        expect(config).toContain("tenant_code: test-tenant");
        expect(config).toContain("workspace_code: test-ws");
    });

    it("surfaces an error on an unreachable endpoint", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".omni/.keep": "" },
        });

        // Port 1 refuses connections, so the access check fails fast.
        const result = await runOmni(
            [
                "cache",
                "remote",
                "setup",
                "-b",
                "http://127.0.0.1:1",
                "-a",
                "test-key",
                "-t",
                "test-tenant",
                "-o",
                "test-org",
                "-w",
                "test-ws",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("failed to setup remote caching");
        expect(ws.exists(".omni/remote-cache.omni.yaml")).toBe(false);
    });
});

describe("+cache @cache @e2e (flaky failure superseded by a later success)", () => {
    /**
     * The task's outcome is driven by `FLAKY_SHOULD_FAIL`, an env var that is
     * never declared as a cache-key input, so it does not enter the task
     * digest. A failure and a later success therefore resolve to the same
     * content-addressed cache entry, exercising the failure -> success
     * replacement path in `HybridTaskExecutionCacheStore::cache_many`.
     */
    const flakyProjectSpec: WorkspaceSpec = {
        workspace: { projects: ["**"] },
        projects: {
            app: { name: "app", tasks: { flaky: "node flaky.js" } },
        },
        files: {
            "app/flaky.js": [
                "if (process.env.FLAKY_SHOULD_FAIL === '1') {",
                "  console.error('flaky: intentional failure');",
                "  process.exit(1);",
                "}",
                "console.log('flaky: success');",
                "",
            ].join("\n"),
        },
    };

    it("a `-f=failed` re-run that succeeds replaces the cached failure", async () => {
        const ws = makeWorkspace(flakyProjectSpec);

        // The task inherits the parent env, so the control var reaches it
        // without being tracked; `-i` keeps that behavior explicit.
        const failed = await runOmni(["-i", "run", "flaky"], {
            cwd: ws.cwd,
            env: { FLAKY_SHOULD_FAIL: "1" },
        });
        expect(failed).toHaveFailed();

        // `-f=failed` ignores the cached failure and re-runs; with the control
        // var unset the task now succeeds and its success must be persisted.
        const recovered = await runOmni(["-i", "run", "flaky", "-f=failed"], {
            cwd: ws.cwd,
        });
        expect(recovered).toHaveSucceeded();
        expect(recovered.stdout).not.toContain("Cache hit for task");

        // With the success cached, another `-f=failed` run serves it from the
        // cache instead of re-executing. The control var would fail a fresh
        // run, so a success here proves the cached success superseded the
        // earlier failure (before the fix the failure stayed sticky and the
        // task was re-run every time).
        const served = await runOmni(["-i", "run", "flaky", "-f=failed"], {
            cwd: ws.cwd,
            env: { FLAKY_SHOULD_FAIL: "1" },
        });
        expect(served).toHaveSucceeded();
        expect(served).toOutputContaining("Cache hit for task 'app#flaky'");
    });

    it("a later failure never overwrites a cached success", async () => {
        const ws = makeWorkspace(flakyProjectSpec);

        const success = await runOmni(["-i", "run", "flaky"], { cwd: ws.cwd });
        expect(success).toHaveSucceeded();

        // Force a fresh (failing) execution of the same digest. `--force=all`
        // re-runs regardless of the cached outcome, and the failing result
        // must not replace the cached success.
        const forcedFailure = await runOmni(["-i", "run", "flaky", "-f"], {
            cwd: ws.cwd,
            env: { FLAKY_SHOULD_FAIL: "1" },
        });
        expect(forcedFailure).toHaveFailed();

        // A subsequent `-f=failed` run still finds the success in the cache.
        const served = await runOmni(["-i", "run", "flaky", "-f=failed"], {
            cwd: ws.cwd,
            env: { FLAKY_SHOULD_FAIL: "1" },
        });
        expect(served).toHaveSucceeded();
        expect(served).toOutputContaining("Cache hit for task 'app#flaky'");
    });
});

describe("+cache @cache @output @e2e (output artifact caching)", () => {
    /**
     * Writes each path passed as an argument (relative to the task's working
     * directory) with a fresh random body. The randomness is the crux of these
     * tests: a genuine cache restore reproduces the *original* body byte-for-
     * byte, whereas a re-execution would emit a different one.
     */
    const artifactWriter = [
        'const fs = require("node:fs");',
        'const path = require("node:path");',
        "for (const rel of process.argv.slice(2)) {",
        "  const abs = path.join(process.cwd(), rel);",
        "  fs.mkdirSync(path.dirname(abs), { recursive: true });",
        // biome-ignore lint/suspicious/noTemplateCurlyInString: expected to be a raw string template literal
        "  fs.writeFileSync(abs, `artifact ${Date.now()}-${Math.random()}`);",
        "}",
        'console.log("built artifact");',
        "",
    ].join("\n");

    it("restores a task's cached output file on a cache hit", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: "node build.js dist/artifact.txt",
                            cache: { output: { files: ["dist/**/*"] } },
                        },
                    },
                },
            },
            files: { "app/build.js": artifactWriter },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const artifact = "app/dist/artifact.txt";
        const original = ws.read(artifact);
        rmSync(ws.path(artifact));
        expect(ws.exists(artifact)).toBe(false);

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hit for task 'app#build'");

        // The task body is random per run, so a matching restored body proves
        // the file came from the cache rather than a re-execution.
        expect(ws.exists(artifact)).toBe(true);
        expect(ws.read(artifact)).toBe(original);
    });

    it("restores files declared by a project-wide `cache.output`", async () => {
        // The task carries no `output` of its own; the artifact is caught,
        // cached, and restored solely via the project-level `cache.output`.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    cache: { output: { files: ["dist/**/*"] } },
                    tasks: { build: "node build.js dist/artifact.txt" },
                },
            },
            files: { "app/build.js": artifactWriter },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const artifact = "app/dist/artifact.txt";
        const original = ws.read(artifact);
        rmSync(ws.path(artifact));

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hit for task 'app#build'");

        expect(ws.exists(artifact)).toBe(true);
        expect(ws.read(artifact)).toBe(original);
    });

    it("appends a task's `cache.output` files to the project-wide ones", async () => {
        // The project catches `shared/**/*`; the task appends `dist/**/*`. Both
        // artifacts must be cached and restored, proving the task list adds to
        // (rather than replaces) the project list.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    cache: { output: { files: ["shared/**/*"] } },
                    tasks: {
                        build: {
                            exec: "node build.js dist/app.txt shared/lib.txt",
                            cache: {
                                output: { files: { append: ["dist/**/*"] } },
                            },
                        },
                    },
                },
            },
            files: { "app/build.js": artifactWriter },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const taskArtifact = "app/dist/app.txt";
        const projectArtifact = "app/shared/lib.txt";
        const originalTask = ws.read(taskArtifact);
        const originalProject = ws.read(projectArtifact);
        rmSync(ws.path(taskArtifact));
        rmSync(ws.path(projectArtifact));

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hit for task 'app#build'");

        expect(ws.read(taskArtifact)).toBe(originalTask);
        expect(ws.read(projectArtifact)).toBe(originalProject);
    });
});
