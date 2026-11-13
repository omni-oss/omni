/**
 * Field-wise task composition across `extends` (RFC 0024 R05), end to end.
 *
 * A task written as a bare command in one file and as a block in another
 * combines field by field instead of the later file replacing the task.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function spec(
    base: unknown,
    child: unknown,
    extra: Record<string, unknown> = {},
): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "base.omni.yaml": {
                name: "base",
                base: true,
                tasks: { build: base },
            },
            app: {
                name: "app",
                extends: ["../base.omni.yaml"],
                tasks: { build: child },
                ...extra,
            },
        },
    };
}

async function printTask(s: WorkspaceSpec) {
    const ws = makeWorkspace(s);
    const result = await runOmni(["project", "print-config", "app", "-r"], {
        cwd: ws.cwd,
    });
    expect(result).toHaveSucceeded();
    const parsed = JSON.parse(result.stdout) as {
        tasks: Record<string, Record<string, unknown>>;
    };
    return { ws, task: parsed.tasks.build as Record<string, unknown> };
}

describe("+task-composition @e2e (R05 shorthand and block)", () => {
    it("a block over a bare command keeps the command and implicit dependency", async () => {
        const { task } = await printTask(
            spec("echo base", { env: { vars: { PROFILE: "release" } } }),
        );
        const json = JSON.stringify(task);

        expect(json).toContain("echo base");
        expect(json).toContain("PROFILE");
        expect(json).toContain("build"); // implicit ^build dependency
        expect(task.dependencies).toBeDefined();
    });

    it("a plain dependencies list drops the implicit dependency", async () => {
        const { task } = await printTask(
            spec("echo base", { dependencies: [] }),
        );

        expect(JSON.stringify(task.dependencies)).not.toContain("build");
    });

    it("a bare command over a block keeps the block's other fields", async () => {
        const { task } = await printTask(
            spec({ exec: "echo old", env: { vars: { A: "1" } } }, "echo new"),
        );
        const json = JSON.stringify(task);

        expect(json).toContain("echo new");
        expect(json).not.toContain("echo old");
        expect(json).toContain('"A"');
    });

    it("two bare commands still replace one another", async () => {
        const { ws } = await printTask(spec("echo one-only", "echo two"));
        const result = await runOmni(["run", "build", "--output-logs", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("two");
    });

    it("a block over a bare command still runs the inherited command", async () => {
        const ws = makeWorkspace(
            spec('echo "inherited"', { env: { vars: { X: "1" } } }),
        );
        const result = await runOmni(["run", "build", "--output-logs", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("inherited");
    });
});

describe("+task-composition @e2e (R05 malformed declarations)", () => {
    it("rejects an alias mixed with task settings", async () => {
        const ws = makeWorkspace(
            spec("echo base", { alias: "other", exec: "echo x" }),
        );
        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
    });

    it("reports an extends cycle", async () => {
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
        const result = await runOmni(["run", "a"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
    });
});
