/**
 * `omni run` output-logs e2e tests: the `--output-logs` and
 * `--output-cached-logs` verbosity flags and the `--ui progress` mode.
 *
 * Pinned to `crates/omni_task_output_logs` (the `LogsDisplay` policy),
 * `crates/omni_cli_core/src/commands/common_args.rs` (the flags), and
 * `crates/omni_cli_core/src/subscriber.rs` (capture + replay).
 *
 * The default policy is `failed`: fresh output is captured and shown only when
 * the task fails, so these tests pass explicit flags to make behavior
 * deterministic regardless of whether stdout is a TTY.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/** A workspace whose single task prints a marker and succeeds. */
function okSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: { name: "app", tasks: { build: 'echo "OUT-MARK"' } },
        },
    };
}

/** A workspace whose single task prints a marker and then fails. */
function failSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: { build: 'sh -c "echo FAIL-MARK; exit 1"' },
            },
        },
    };
}

describe("+run @output-logs (fresh output policy)", () => {
    it("`--output-logs all` shows fresh output on success", async () => {
        const ws = makeWorkspace(okSpec());

        const result = await runOmni(["run", "build", "--output-logs", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });

    it("`--output-logs never` suppresses fresh output on success", async () => {
        const ws = makeWorkspace(okSpec());

        const result = await runOmni(
            ["run", "build", "--output-logs", "never"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("OUT-MARK");
    });

    it("`--output-logs failed` hides output for a successful task", async () => {
        const ws = makeWorkspace(okSpec());

        const result = await runOmni(
            ["run", "build", "--output-logs", "failed"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("OUT-MARK");
    });

    it("`--output-logs failed` replays output for a failing task", async () => {
        const ws = makeWorkspace(failSpec());

        const result = await runOmni(
            ["run", "build", "--output-logs", "failed"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result).toOutputContaining("FAIL-MARK");
    });

    it("`--output-logs never` suppresses output even for a failing task", async () => {
        const ws = makeWorkspace(failSpec());

        const result = await runOmni(
            ["run", "build", "--output-logs", "never"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result.stdout).not.toContain("FAIL-MARK");
    });
});

describe("+run @output-logs (cached output policy)", () => {
    it("`--output-cached-logs all` replays a cache hit", async () => {
        const ws = makeWorkspace(okSpec());

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(
            ["run", "build", "--output-cached-logs", "all"],
            { cwd: ws.cwd },
        );
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second).toOutputContaining("OUT-MARK");
    });

    it("`--output-cached-logs never` skips replay on a cache hit", async () => {
        const ws = makeWorkspace(okSpec());

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(
            ["run", "build", "--output-cached-logs", "never"],
            { cwd: ws.cwd },
        );
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second.stdout).not.toContain("OUT-MARK");
    });
});

describe("+run @output-logs (progress ui)", () => {
    it("`--ui progress` runs successfully and shows output with --output-logs all", async () => {
        const ws = makeWorkspace(okSpec());

        // Off a TTY (as under the test harness) `progress` degrades to `stream`;
        // the run still completes and honors the output-logs policy.
        const result = await runOmni(
            ["run", "build", "--ui", "progress", "--output-logs", "all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });
});

describe("+run @output-logs (config + flag precedence)", () => {
    it("task-level `output_logs: all` shows fresh output without a flag", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: { exec: 'echo "OUT-MARK"', output_logs: "all" },
                    },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });

    it("project-level `output_logs: all` applies to its tasks", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    output_logs: "all",
                    tasks: { build: 'echo "OUT-MARK"' },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });

    it("task-level `output_logs` overrides the project-level value", async () => {
        // Project says `never`, the task says `all`: the task wins per facet and
        // the fresh output is shown even without a flag.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    output_logs: "never",
                    tasks: {
                        build: { exec: 'echo "OUT-MARK"', output_logs: "all" },
                    },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });

    it("project-level `output_logs: never` suppresses a task with no override", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    output_logs: "never",
                    tasks: { build: 'echo "OUT-MARK"' },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("OUT-MARK");
    });

    it("`--output-logs` flag overrides the resolved config", async () => {
        // The config asks to always show; the flag forces `never` and wins.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: { exec: 'echo "OUT-MARK"', output_logs: "all" },
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "build", "--output-logs", "never"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("OUT-MARK");
    });

    it("split `output_logs: { new: all }` shows fresh output without a flag", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "OUT-MARK"',
                            output_logs: { new: "all", cached: "never" },
                        },
                    },
                },
            },
        });

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OUT-MARK");
    });
});

describe("+run @output-logs (cached config + flag precedence)", () => {
    it("task-level `output_logs: all` replays a cache hit without a flag", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: { exec: 'echo "OUT-MARK"', output_logs: "all" },
                    },
                },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        // The `cached` facet resolves to `all`, so the cache hit is replayed.
        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second).toOutputContaining("OUT-MARK");
    });

    it("task-level `output_logs: { cached: never }` skips replay without a flag", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "OUT-MARK"',
                            output_logs: { cached: "never" },
                        },
                    },
                },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second.stdout).not.toContain("OUT-MARK");
    });

    it("task-level `cached` overrides the project-level `cached` value", async () => {
        // Project disables cached replay; the task re-enables it and wins.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    output_logs: { cached: "never" },
                    tasks: {
                        build: {
                            exec: 'echo "OUT-MARK"',
                            output_logs: { cached: "all" },
                        },
                    },
                },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second).toOutputContaining("OUT-MARK");
    });

    it("`--output-cached-logs` flag overrides the config cached facet", async () => {
        // Config asks to always replay; the flag forces `never` for the cache hit.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: { exec: 'echo "OUT-MARK"', output_logs: "all" },
                    },
                },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(
            ["run", "build", "--output-cached-logs", "never"],
            { cwd: ws.cwd },
        );
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second.stdout).not.toContain("OUT-MARK");
    });

    it("`--output-logs all` propagates to the cached facet, overriding config", async () => {
        // Config disables cached replay; the uniform flag re-enables it because
        // `--output-cached-logs` falls back to `--output-logs`.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "OUT-MARK"',
                            output_logs: { cached: "never" },
                        },
                    },
                },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(["run", "build", "--output-logs", "all"], {
            cwd: ws.cwd,
        });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("Cache hits");
        expect(second).toOutputContaining("OUT-MARK");
    });
});
