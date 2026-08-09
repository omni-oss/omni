/**
 * Task-level extension e2e tests: `base` (hidden template) and `extends`
 * (inherit + override sibling tasks) inside a project's `tasks` map.
 *
 * Task extension is resolved after the project extension graph is merged, so a
 * task can extend a `base` task inherited from a parent project. Exercised
 * through the built binary via `project print-config` (the resolved, merged
 * config) and `run` (actual task execution).
 *
 * Pinned to `crates/omni_configurations/src/task_extension.rs` and the pipeline
 * wiring in `crates/omni_context/src/context.rs`.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni } from "@/harness";

/** A single project with a hidden `base` task and a task that extends it. */
function baseAndDerivedSpec() {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: {
                    "base-task": {
                        base: true,
                        exec: 'echo "from base"',
                    },
                    derived: {
                        extends: "base-task",
                    },
                },
            },
        },
    };
}

describe("+task-extension @e2e (base / extends)", () => {
    it("drops the base task and merges it into the deriving task", async () => {
        const ws = makeWorkspace(baseAndDerivedSpec());

        const result = await runOmni(["project", "print-config", "app", "-r"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        const parsed = JSON.parse(result.stdout) as {
            tasks: Record<string, unknown>;
        };

        // The base task is hidden; the deriving task survives.
        expect(parsed.tasks).not.toHaveProperty("base-task");
        expect(parsed.tasks).toHaveProperty("derived");

        // The deriving task inherited the base task's command, and no residual
        // `base`/`extends` markers remain.
        const derived = JSON.stringify(parsed.tasks.derived);
        expect(derived).toContain("from base");
        expect(derived).not.toContain("extends");
        expect(derived).not.toContain('"base"');
    });

    it("runs the command inherited from the base task", async () => {
        const ws = makeWorkspace(baseAndDerivedSpec());

        const result = await runOmni(
            ["run", "derived", "-p", "app", "--output-logs=all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("from base");
    });

    it("a base task is not runnable", async () => {
        const ws = makeWorkspace(baseAndDerivedSpec());

        const result = await runOmni(["run", "base-task", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no task to execute");
    });

    it("accepts `extends` as a list of task names", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        "base-task": { base: true, exec: 'echo "from base"' },
                        derived: { extends: ["base-task"] },
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "derived", "-p", "app", "--output-logs=all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("from base");
    });

    it("extends a base task inherited from a parent project", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["nested/**"] },
            projects: {
                "base.omni.yaml": {
                    name: "parent",
                    base: true,
                    tasks: {
                        "base-task": { base: true, exec: 'echo "from parent"' },
                    },
                },
                "nested/child": {
                    name: "child",
                    extends: ["../../base.omni.yaml"],
                    tasks: { derived: { extends: "base-task" } },
                },
            },
        });

        const result = await runOmni(
            ["run", "derived", "-p", "child", "--output-logs=all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("from parent");
    });
});

describe("+task-extension @exitcode (errors)", () => {
    it("rejects an `extends` reference to an unknown task", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { derived: { extends: "missing", exec: "echo hi" } },
                },
            },
        });

        const result = await runOmni(["project", "list"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("extends unknown task 'missing'");
    });

    it("rejects a cycle in task `extends`", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        a: { extends: "b", exec: "echo a" },
                        b: { extends: "a", exec: "echo b" },
                    },
                },
            },
        });

        const result = await runOmni(["project", "list"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("cyclic task extension detected");
    });
});
