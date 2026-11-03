/**
 * Meta filter runtime errors (RFC 0020): a `--meta` expression that fails to
 * evaluate for a task is reported instead of silently excluding the task.
 * Pinned to `crates/omni_execution_plan/src/filter.rs` and
 * `crates/omni_execution_plan/src/meta_filter_error.rs`.
 */

import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    runOmni,
    type WorkspaceSpec,
} from "@/harness";

/** `web` (kind: unit), `api` (kind: e2e) and `docs` (no meta at all). */
function metaSpec(): WorkspaceSpec {
    const task = (name: string, kind?: string) => ({
        exec: `echo "RAN-${name}"`,
        ...(kind ? { meta: { kind } } : {}),
    });
    return {
        workspace: { projects: ["**"] },
        projects: {
            web: { name: "web", tasks: { test: task("web", "unit") } },
            api: { name: "api", tasks: { test: task("api", "e2e") } },
            docs: { name: "docs", tasks: { test: task("docs") } },
        },
    };
}

function manyProjectsSpec(count: number): WorkspaceSpec {
    const projects: WorkspaceSpec["projects"] = {};
    for (let i = 0; i < count; i++) {
        projects[`p${i}`] = {
            name: `p${i}`,
            tasks: { test: { exec: "echo ok", meta: { kind: "unit" } } },
        };
    }
    return { workspace: { projects: ["**"] }, projects };
}

describe("+meta-filter-errors @e2e (cli)", () => {
    it("a typo fails with the filter, cause, samples and a hint", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(["run", "test", "--meta", 'knd == "e2e"'], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        const out = result.stdout + result.stderr;
        expect(out).toContain('knd == "e2e"');
        expect(out).toContain("Undeclared reference to 'knd'");
        expect(out).toContain("[3 tasks]");
        expect(out).toContain("web#test");
        expect(out).toContain("api#test");
        expect(out).toContain("docs#test");
        expect(out).toContain('"kind": "unit"');
        expect(out).toContain("Did you mean `kind`?");
        expect(out).not.toContain("RAN-");
        expect(out).toContain("error[meta_filter.evaluation]");
    });

    it("exits 2 and prints JSON with --error-format json", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(
            ["run", "test", "--meta", 'knd == "e2e"', "--error-format", "json"],
            { cwd: ws.cwd },
        );

        expect(result.exitCode).toBe(2);
        const line = result.stderr
            .split("\n")
            .find((l) => l.startsWith("{"));
        const body = JSON.parse(line ?? "{}");
        expect(body.kind).toBe("meta_filter.evaluation");
        expect(body.audience).toBe("user");
        expect(body.details.total).toBe(3);
    });

    it("a key missing on some tasks fails, naming the task without meta", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(["run", "test", "--meta", 'kind == "e2e"'], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        const out = result.stdout + result.stderr;
        expect(out).toContain("docs#test");
        expect(out).toContain("Undeclared reference to 'kind'");
    });

    it("`has(meta.key)` makes the key optional", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(
            [
                "run",
                "test",
                "--meta",
                'has(meta.kind) && meta.kind == "e2e"',
                "--output-logs=all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("RAN-api");
        expect(result.stdout).not.toContain("RAN-web");
        expect(result.stdout).not.toContain("RAN-docs");
    });

    it("a type error fails with the cause", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(["run", "test", "--meta", "kind > 1"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result.stdout + result.stderr).toContain("Operand types differ");
    });

    it("a non-bool result fails naming the type", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(
            ["run", "test", "--meta", "has(meta.kind) ? meta.kind : false"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result.stdout + result.stderr).toContain("got string");
    });

    it("`--meta-on-error exclude` warns and runs the remaining tasks", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(
            [
                "run",
                "test",
                "--meta",
                'kind == "e2e"',
                "--meta-on-error",
                "exclude",
                "--output-logs=all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        const out = result.stdout + result.stderr;
        expect(out).toContain("they were excluded");
        expect(out).toContain("docs#test");
        expect(out).toContain("RAN-api");
        expect(out).not.toContain("RAN-docs");
    });

    it("`--meta-error-samples` bounds the listed tasks but not the total", async () => {
        const ws = makeWorkspace(manyProjectsSpec(20));

        const run = (samples: string) =>
            runOmni(
                ["run", "test", "--meta", "knd == 1", "--meta-error-samples", samples],
                { cwd: ws.cwd },
            );

        const def = await runOmni(["run", "test", "--meta", "knd == 1"], {
            cwd: ws.cwd,
        });
        expect(def).toHaveFailed();
        expect(def.stdout + def.stderr).toContain("20 task(s)");
        expect(def.stdout + def.stderr).toContain("and 17 more");

        const one = await run("1");
        expect(one.stdout + one.stderr).toContain("and 19 more");

        const zero = await run("0");
        expect(zero).toHaveFailed();
        expect(zero.stdout + zero.stderr).toContain("[20 tasks]");
        expect(zero.stdout + zero.stderr).not.toContain("#test");

        const bad = await run("-1");
        expect(bad).toHaveFailed();
    });

    it("a valid filter over complete meta behaves as before", async () => {
        const ws = makeWorkspace(metaSpec());

        const result = await runOmni(
            [
                "run",
                "test",
                "--meta",
                'kind == "unit"',
                "-p",
                "web",
                "--output-logs=all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("RAN-web");
    });
});

describe("+meta-filter-errors @e2e @mcp (task_run)", { tags: ["mcp"] }, () => {
    it("returns a structured error for a failing meta filter", async () => {
        const ws = makeWorkspace(metaSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const error = (await client
            .callTool({
                name: "task_run",
                arguments: { tasks: ["test"], meta: 'knd == "e2e"' },
            })
            .then(
                () => undefined,
                (e: unknown) => e,
            )) as { code: number; message: string; data: any } | undefined;

        expect(error).toBeDefined();
        expect(error?.code).toBe(-32602);
        expect(error?.data.kind).toBe("meta_filter.evaluation");
        expect(error?.data.audience).toBe("user");
        expect(error?.data.details.total).toBe(3);
        expect(error?.data.filter).toBe('knd == "e2e"');
        expect(error?.data.total).toBe(3);
        const group = error?.data.groups[0];
        expect(group.cause).toContain("Undeclared reference to 'knd'");
        expect(group.count).toBe(3);
        expect(group.samples).toHaveLength(3);
        expect(group.hint).toContain("kind");
        // Deprecated top-level fields (RFC 0021, section 4.4).
        expect(error?.data.other).toBe(0);
    });

    it("`meta_on_error: exclude` runs the remaining tasks", async () => {
        const ws = makeWorkspace(metaSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "task_run",
            arguments: {
                tasks: ["test"],
                meta: 'kind == "e2e"',
                meta_on_error: "exclude",
            },
        });

        const data = result.structuredContent as {
            ok: boolean;
            results: Array<{ project: string }>;
        };
        expect(data.ok).toBe(true);
        expect(data.results.map((r) => r.project)).toEqual(["api"]);
    });
});
