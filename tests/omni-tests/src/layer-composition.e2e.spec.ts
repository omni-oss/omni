/**
 * Presence-aware project layers (RFC 0024 R04), end to end.
 *
 * An omitted field inherits through `extends`; only an explicit value
 * overrides. Defaults and the required `name` apply after composition.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function logsSpec(childCache: Record<string, unknown>): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "base.omni.yaml": {
                name: "base",
                base: true,
                cache: { output: { logs: false } },
            },
            app: {
                name: "app",
                extends: ["../base.omni.yaml"],
                cache: childCache,
                tasks: { build: 'echo "built app"' },
            },
        },
    };
}

async function replay(spec: WorkspaceSpec) {
    const ws = makeWorkspace(spec);
    expect(await runOmni(["run", "build"], { cwd: ws.cwd })).toHaveSucceeded();
    const second = await runOmni(
        ["run", "build", "--output-cached-logs", "all"],
        { cwd: ws.cwd },
    );
    expect(second).toHaveSucceeded();
    expect(second).toHaveStderrContaining("Cache hit for task 'app#build'");
    return second;
}

describe("+config @e2e (R04 absent vs explicit across extends)", () => {
    it("a child that omits cache.output.logs keeps the base's false", async () => {
        const second = await replay(logsSpec({ key: { env: ["E"] } }));

        expect(second.stderr).not.toContain("(replaying logs)");
    });

    it("a child that sets cache.output.logs true overrides the base", async () => {
        const second = await replay(logsSpec({ output: { logs: true } }));

        expect(second).toHaveStderrContaining("(replaying logs)");
    });
});

describe("+config @e2e (R04 name is validated after composition)", () => {
    it("a child may take its name from the file it extends", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "base.omni.yaml": { name: "shared", base: true },
                app: {
                    extends: ["../base.omni.yaml"],
                    tasks: { build: 'echo "built shared"' },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("shared#build");
    });

    it("a project with no name anywhere fails to load", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: { app: { tasks: { build: 'echo "x"' } } },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).not.toHaveSucceeded();
        expect(result.stderr).toContain("has no `name`");
    });
});
