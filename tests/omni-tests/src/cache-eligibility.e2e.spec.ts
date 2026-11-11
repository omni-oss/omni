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

        ws.write("app/project.omni.yaml", appSpec(false).projects!.app);

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
