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
});
