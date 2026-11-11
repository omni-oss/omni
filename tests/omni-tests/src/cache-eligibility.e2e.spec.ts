/**
 * Cache eligibility and composition regressions fixed by RFC 0024 B01-B03,
 * end to end.
 *
 * - B01: disabling caching must stop cache reads, not only writes.
 * - B02: a layer that omits one glob side must not wipe the inherited side.
 * - B03: an operator object with sibling keys is rejected, not partly dropped.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const RAW_HASH_PATTERN = /^[A-Za-z0-9]{40,}$/;

function appSpec(enabled: boolean): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                // Keep the config file out of the cache key so flipping
                // `enabled` is the only change between the two runs.
                cache: { enabled, key: { defaults: false, files: ["src/**"] } },
                tasks: { build: 'echo "built"' },
            },
        },
        files: { "app/src/a.txt": "a\n" },
    };
}

describe("+cache @cache @e2e (B01 lookup honors cache.enabled)", () => {
    it("does not replay an existing entry once caching is disabled", async () => {
        const ws = makeWorkspace(appSpec(true));

        expect(
            await runOmni(["run", "build"], { cwd: ws.cwd }),
        ).toHaveSucceeded();
        const hit = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(hit).toHaveSucceeded();
        expect(hit).toHaveStderrContaining("Cache hit for task 'app#build'");

        ws.write("app/project.omni.yaml", appSpec(false).projects!.app!);

        const disabled = await runOmni(
            ["run", "build", "--output-logs", "all"],
            { cwd: ws.cwd },
        );
        expect(disabled).toHaveSucceeded();
        expect(disabled.stderr).not.toContain("Cache hit for task");
        expect(disabled).toOutputContaining("built");
    });

    it("keeps hitting the entry while caching stays enabled", async () => {
        const ws = makeWorkspace(appSpec(true));

        expect(
            await runOmni(["run", "build"], { cwd: ws.cwd }),
        ).toHaveSucceeded();
        const hit = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(hit).toHaveStderrContaining("Cache hit for task 'app#build'");
    });
});

function layeredGlobSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "base.omni.yaml": {
                name: "base",
                base: true,
                cache: {
                    key: {
                        defaults: false,
                        files: {
                            include: ["src/**"],
                            exclude: ["src/gen/**"],
                        },
                    },
                },
            },
            app: {
                name: "app",
                extends: ["../base.omni.yaml"],
                // Only the include side is restated; the base exclude must
                // survive the merge.
                cache: {
                    key: { files: { include: { append: ["extra/**"] } } },
                },
                tasks: { build: 'echo "build"' },
            },
        },
        files: {
            "app/src/a.txt": "a\n",
            "app/src/gen/b.txt": "b\n",
            "app/extra/c.txt": "c\n",
        },
    };
}

async function projectHash(cwd: string): Promise<string> {
    const result = await runOmni(
        ["hash", "-r", "project", "app", "-t", "build"],
        { cwd },
    );
    expect(result).toHaveSucceeded();
    expect(result.stdout).toMatch(RAW_HASH_PATTERN);
    return result.stdout;
}

describe("+hashing @e2e (B02 glob side presence across extends)", () => {
    it("keeps the inherited exclude when a child only extends the include", async () => {
        const ws = makeWorkspace(layeredGlobSpec());
        const base = await projectHash(ws.cwd);

        ws.write("app/src/gen/b.txt", "b changed\n");
        expect(await projectHash(ws.cwd)).toBe(base);

        ws.write("app/extra/c.txt", "c changed\n");
        const afterExtra = await projectHash(ws.cwd);
        expect(afterExtra).not.toBe(base);

        ws.write("app/src/a.txt", "a changed\n");
        expect(await projectHash(ws.cwd)).not.toBe(afterExtra);
    });
});

describe("+config @e2e (B03 strict operator envelopes)", () => {
    it("rejects an operator object with a sibling key", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        merge: { build: 'echo "build"' },
                        hidden: 'echo "hidden"',
                    },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("failed to parse YAML file at `tasks`");
    });

    it("accepts a task named like an operator inside an envelope", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { merge: { replace: 'echo "replaced"' } },
                },
            },
        });

        const result = await runOmni(["run", "replace"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
    });
});

function depSpec(tasks: Record<string, unknown>): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                // Keep the config file out of every cache key.
                cache: { key: { defaults: false, files: ["src/**"] } },
                tasks,
            },
        },
        files: { "app/src/a.txt": "a\n" },
    };
}

const UNCACHED = { exec: 'echo "gen"', cache: { enabled: false } };

async function runTwice(
    tasks: Record<string, unknown>,
    target: string,
): Promise<{ second: Awaited<ReturnType<typeof runOmni>> }> {
    const ws = makeWorkspace(depSpec(tasks));
    expect(await runOmni(["run", target], { cwd: ws.cwd })).toHaveSucceeded();
    const second = await runOmni(["run", target], { cwd: ws.cwd });
    expect(second).toHaveSucceeded();
    return { second };
}

describe("+cache @cache @e2e (B04 uncached dependency demotion)", () => {
    it("a cache-enabled task is not reused after an uncached dependency", async () => {
        const { second } = await runTwice(
            {
                gen: UNCACHED,
                build: { exec: 'echo "build"', dependencies: ["gen"] },
            },
            "build",
        );

        expect(second.stderr).not.toContain("Cache hit for task 'app#build'");
        expect(second).toHaveStderrContaining("ran uncached");
    });

    it("demotion propagates along a dependency chain", async () => {
        const { second } = await runTwice(
            {
                gen: UNCACHED,
                mid: { exec: 'echo "mid"', dependencies: ["gen"] },
                top: { exec: 'echo "top"', dependencies: ["mid"] },
            },
            "top",
        );

        expect(second.stderr).not.toContain("Cache hit for task 'app#mid'");
        expect(second.stderr).not.toContain("Cache hit for task 'app#top'");
    });

    it("a commandless aggregate node is transparent", async () => {
        const { second } = await runTwice(
            {
                gen: 'echo "gen"',
                agg: { dependencies: ["gen"] },
                build: { exec: 'echo "build"', dependencies: ["agg"] },
            },
            "build",
        );

        expect(second).toHaveStderrContaining("Cache hit for task 'app#build'");
        expect(second.stderr).not.toContain("ran uncached");
    });

    it("an uncached task behind an aggregate node still demotes", async () => {
        const { second } = await runTwice(
            {
                gen: UNCACHED,
                agg: { dependencies: ["gen"] },
                build: { exec: 'echo "build"', dependencies: ["agg"] },
            },
            "build",
        );

        expect(second.stderr).not.toContain("Cache hit for task 'app#build'");
    });

    it("cached dependencies and unrelated tasks keep hitting", async () => {
        const { second } = await runTwice(
            {
                gen: 'echo "gen"',
                build: { exec: 'echo "build"', dependencies: ["gen"] },
                other: UNCACHED,
                free: 'echo "free"',
            },
            "build",
        );

        expect(second).toHaveStderrContaining("Cache hit for task 'app#gen'");
        expect(second).toHaveStderrContaining("Cache hit for task 'app#build'");
    });

    it("a change behind a commandless aggregate node still invalidates dependents", async () => {
        const ws = makeWorkspace({
            ...depSpec({
                gen: {
                    exec: 'echo "gen"',
                    cache: { key: { defaults: false, files: ["gen-in/**"] } },
                },
                agg: { dependencies: ["gen"] },
                build: {
                    exec: 'echo "build"',
                    dependencies: ["agg"],
                    cache: { key: { defaults: false, files: ["build-in/**"] } },
                },
            }),
            files: { "app/gen-in/a.txt": "a\n", "app/build-in/b.txt": "b\n" },
        });
        const run = () => runOmni(["run", "build"], { cwd: ws.cwd });

        expect(await run()).toHaveSucceeded();
        expect(await run()).toHaveStderrContaining(
            "Cache hit for task 'app#build'",
        );

        ws.write("app/gen-in/a.txt", "changed\n");

        const rerun = await run();
        expect(rerun).toHaveSucceeded();
        expect(rerun.stderr).not.toContain("Cache hit for task 'app#build'");
    });
});
