/**
 * RFC 0024 F03.4 slice 4: `cache stats` and `cache prune` match stored
 * canonical names only. An exact task selector that matches no stored entry but
 * names a live alias gets an advisory hint; the hint never changes which
 * entries are selected, pruned or approved.
 */

import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    runOmni,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";

function webTasks(extra: Record<string, unknown> = {}) {
    return {
        "build-native": 'echo "native"',
        lint: 'echo "lint"',
        ...extra,
    };
}

function spec(tasks: Record<string, unknown>): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            web: { name: "web", tasks: tasks as never },
        },
    };
}

async function seed(ws: Workspace, ...tasks: string[]) {
    expect(
        await runOmni(["run", ...tasks, "-p", "web"], { cwd: ws.cwd }),
    ).toHaveSucceeded();
}

function rewrite(ws: Workspace, tasks: Record<string, unknown>) {
    ws.write("web/project.omni.yaml", JSON.stringify({ name: "web", tasks }));
}

const HINT = "Hint: no cache entries are stored under `compile`";

describe("+cache @e2e (alias hints are advisory and canonical-only)", () => {
    it("an alias-only exact miss yields a hint and selects nothing", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));

        const stats = await runOmni(
            ["cache", "stats", "--project", "web", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(stats).toHaveSucceeded();
        expect(stats.stdout).not.toContain("- Task:");
        expect(stats.stdout).toContain(HINT);
        expect(stats.stdout).toContain("`--project web --task build-native`");

        const canonical = await runOmni(
            ["cache", "stats", "--project", "web", "--task", "build-native"],
            { cwd: ws.cwd },
        );
        expect(canonical.stdout).toContain("- Task: build-native");
        expect(canonical.stdout).not.toContain("Hint:");
    });

    it("historical canonical names stay selectable and get no hint", async () => {
        const ws = makeWorkspace(spec(webTasks({ compile: 'echo "old"' })));
        await seed(ws, "compile");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));

        const stats = await runOmni(
            ["cache", "stats", "--project", "web", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(stats.stdout).toContain("- Task: compile");
        expect(stats.stdout).not.toContain("Hint:");
    });

    it("a prune by alias removes nothing and still shows the hint", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));

        const dry = await runOmni(
            ["cache", "prune", "--dry-run", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(dry).toHaveSucceeded();
        expect(dry.stdout).toContain("No cache entries matched");
        expect(dry.stdout).toContain(HINT);

        const real = await runOmni(
            ["cache", "prune", "--yes", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(real).toHaveSucceeded();
        expect(real.stdout).toContain(HINT);
        const after = await runOmni(
            ["cache", "stats", "--project", "web", "--task", "build-native"],
            { cwd: ws.cwd },
        );
        expect(after.stdout).toContain("- Task: build-native");
    });

    it("retargeting the alias changes only the hint, never the entries", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native", "lint");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        const first = await runOmni(
            ["cache", "prune", "--dry-run", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(first.stdout).toContain("task `build-native`");

        rewrite(ws, webTasks({ compile: { alias: "lint" } }));
        const second = await runOmni(
            ["cache", "prune", "--dry-run", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(second.stdout).toContain("task `lint`");
        expect(second.stdout).toContain("No cache entries matched");
    });

    it("patterns get no alias expansion or hint", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        const stats = await runOmni(
            ["cache", "stats", "--task-pattern", "comp*"],
            { cwd: ws.cwd },
        );
        expect(stats.stdout).not.toContain("- Task:");
        expect(stats.stdout).not.toContain("Hint:");
    });

    it("an exact hit in one scoped project suppresses the hint for that selector", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                web: { name: "web", tasks: webTasks() as never },
                api: {
                    name: "api",
                    tasks: { compile: 'echo "api compile"' },
                },
            },
        });
        expect(
            await runOmni(["run", "build-native", "compile"], { cwd: ws.cwd }),
        ).toHaveSucceeded();
        ws.write(
            "web/project.omni.yaml",
            JSON.stringify({
                name: "web",
                tasks: webTasks({ compile: { alias: "build-native" } }),
            }),
        );
        const stats = await runOmni(["cache", "stats", "--task", "compile"], {
            cwd: ws.cwd,
        });
        expect(stats.stdout).toContain("Project: api");
        expect(stats.stdout).not.toContain("Hint:");
    });

    it("stale and orphan prune keep the hint advisory", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        for (const flag of ["--stale", "--orphan"]) {
            const result = await runOmni(
                ["cache", "prune", "--dry-run", flag, "--task", "compile"],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            expect(result.stdout).toContain("No cache entries matched");
            expect(result.stdout).toContain(HINT);
        }
    });
});

async function stored(ws: Workspace, task: string): Promise<boolean> {
    const stats = await runOmni(
        ["cache", "stats", "--project", "web", "--task", task],
        { cwd: ws.cwd },
    );
    expect(stats).toHaveSucceeded();
    return stats.stdout.includes(`- Task: ${task}`);
}

describe("+cache @e2e (alias hints on destructive prune paths)", () => {
    it("ordinary prune --yes deletes the selected canonical entry", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native", "lint");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        const result = await runOmni(
            ["cache", "prune", "--yes", "--task", "build-native"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("Hint:");
        expect(await stored(ws, "build-native")).toBe(false);
        expect(await stored(ws, "lint")).toBe(true);
    });

    it("mixed selectors: a stored hit suppresses only its own hint", async () => {
        const ws = makeWorkspace(
            spec(webTasks({ old: 'echo "old"', other: 'echo "other"' })),
        );
        await seed(ws, "build-native", "old");
        rewrite(
            ws,
            webTasks({
                old: { alias: "lint" },
                compile: { alias: "build-native" },
            }),
        );
        const preview = await runOmni(
            [
                "cache",
                "prune",
                "--dry-run",
                "--task",
                "old",
                "--task",
                "compile",
            ],
            { cwd: ws.cwd },
        );
        expect(preview).toHaveSucceeded();
        expect(preview.stdout).toContain(HINT);
        expect(preview.stdout).not.toContain("stored under `old`");

        const real = await runOmni(
            ["cache", "prune", "--yes", "--task", "old", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(real).toHaveSucceeded();
        expect(real.stdout).toContain(HINT);
        expect(real.stdout).not.toContain("stored under `old`");
        expect(await stored(ws, "old")).toBe(false);
        expect(await stored(ws, "build-native")).toBe(true);
    });

    it("stale prune --yes deletes only stale canonical entries and never follows an alias", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native", "lint");
        rewrite(
            ws,
            webTasks({
                "build-native": 'echo "changed"',
                compile: { alias: "build-native" },
            }),
        );
        const byAlias = await runOmni(
            ["cache", "prune", "--stale", "--yes", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(byAlias).toHaveSucceeded();
        expect(byAlias.stdout).toContain(HINT);
        expect(await stored(ws, "build-native")).toBe(true);

        // Retargeting must not broaden what the same request removes.
        rewrite(
            ws,
            webTasks({
                "build-native": 'echo "changed"',
                compile: { alias: "lint" },
            }),
        );
        const retargeted = await runOmni(
            ["cache", "prune", "--stale", "--yes", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(retargeted).toHaveSucceeded();
        expect(retargeted.stdout).toContain("task `lint`");
        expect(await stored(ws, "build-native")).toBe(true);
        expect(await stored(ws, "lint")).toBe(true);

        const canonical = await runOmni(
            ["cache", "prune", "--stale", "--yes", "--task", "build-native"],
            { cwd: ws.cwd },
        );
        expect(canonical).toHaveSucceeded();
        expect(canonical.stdout).not.toContain("Hint:");
        expect(await stored(ws, "build-native")).toBe(false);
        expect(await stored(ws, "lint")).toBe(true);
    });

    it("orphan prune --yes deletes a historical entry without a hint and never follows an alias", async () => {
        const ws = makeWorkspace(spec(webTasks({ old: 'echo "old"' })));
        await seed(ws, "build-native", "old");
        rewrite(
            ws,
            webTasks({
                old: { alias: "build-native" },
                compile: { alias: "build-native" },
            }),
        );
        const byAlias = await runOmni(
            ["cache", "prune", "--orphan", "--yes", "--task", "compile"],
            { cwd: ws.cwd },
        );
        expect(byAlias).toHaveSucceeded();
        expect(byAlias.stdout).toContain(HINT);
        expect(await stored(ws, "old")).toBe(true);

        const historical = await runOmni(
            ["cache", "prune", "--orphan", "--yes", "--task", "old"],
            { cwd: ws.cwd },
        );
        expect(historical).toHaveSucceeded();
        expect(historical.stdout).not.toContain("Hint:");
        expect(await stored(ws, "old")).toBe(false);
        expect(await stored(ws, "build-native")).toBe(true);
    });
});

describe("+mcp @mcp @cli (alias hints)", { tags: ["mcp", "caching"] }, () => {
    it("cache_stats and cache_prune carry alias_hints only when there are some", async () => {
        const ws = makeWorkspace(spec(webTasks()));
        await seed(ws, "build-native");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        const { client } = await connectMcp({ cwd: ws.cwd });

        const stats = await client.callTool({
            name: "cache_stats",
            arguments: { project: ["web"], task: ["compile"] },
        });
        const hints = [
            {
                selector: "compile",
                project: "web",
                alias: "compile",
                canonical: "build-native",
            },
        ];
        expect(stats.structuredContent).toMatchObject({ alias_hints: hints });
        const listed = (
            stats.structuredContent as {
                projects: Array<{ tasks: unknown[] }>;
            }
        ).projects;
        expect(listed.flatMap((p) => p.tasks)).toEqual([]);

        const prune = await client.callTool({
            name: "cache_prune",
            arguments: { task: ["compile"], dry_run: true },
        });
        expect(prune.structuredContent).toMatchObject({
            entries_pruned: 0,
            alias_hints: hints,
        });

        const plain = await client.callTool({
            name: "cache_stats",
            arguments: { task: ["build-native"] },
        });
        expect(plain.structuredContent).not.toHaveProperty("alias_hints");
    });

    it("cache_prune dry_run false of a historical entry named by an alias reports the deletion and no hint", async () => {
        const ws = makeWorkspace(spec(webTasks({ compile: 'echo "old"' })));
        await seed(ws, "compile");
        rewrite(ws, webTasks({ compile: { alias: "build-native" } }));
        const { client } = await connectMcp({ cwd: ws.cwd });
        const prune = await client.callTool({
            name: "cache_prune",
            arguments: { task: ["compile"], dry_run: false },
        });
        expect(prune.structuredContent).toMatchObject({
            entries_pruned: 1,
        });
        expect(prune.structuredContent).not.toHaveProperty("alias_hints");
        expect(await stored(ws, "compile")).toBe(false);
    });
});
