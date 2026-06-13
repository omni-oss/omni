/**
 * `omni run` shared filtering/flags (the flattened `RunArgs`, also used by
 * `omni exec`): project/dir/meta filters, dry-run, concurrency, `-a/--arg`
 * injection, UI mode, `--result` output, retry overrides, and the execution
 * summary. Pinned to `crates/omni_cli_core/src/commands/common_args.rs` and
 * `crates/omni_cli_core/src/commands/utils.rs`.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/** Two named projects whose `build` echoes a distinct, project-specific marker. */
function distinctBuildsSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: { name: "app", tasks: { build: 'echo "BUILD-APP"' } },
            other: { name: "other", tasks: { build: 'echo "BUILD-OTHER"' } },
        },
    };
}

describe("+run-filters @e2e (project & dir filters)", () => {
    it("`-p/--project` limits execution to matching projects", async () => {
        const ws = makeWorkspace(distinctBuildsSpec());

        const result = await runOmni(["run", "build", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-APP");
        expect(result.stdout).not.toContain("BUILD-OTHER");
    });

    it("`--dir` limits execution to projects under matching directories", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "svc/api": {
                    name: "api",
                    tasks: { build: 'echo "BUILD-API"' },
                },
                other: {
                    name: "other",
                    tasks: { build: 'echo "BUILD-OTHER"' },
                },
            },
        });

        const result = await runOmni(["run", "build", "--dir", "svc/**"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-API");
        expect(result.stdout).not.toContain("BUILD-OTHER");
    });
});

describe("+run-filters @e2e (dry run)", () => {
    it("`-d/--dry-run` prints the plan without executing tasks", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "DRY-MARK"' } },
            },
        });

        const result = await runOmni(["run", "build", "-d"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Dry run mode enabled");
        expect(result).toOutputContaining("Executing task 'app#build'");
        // The command never runs, so its output is absent.
        expect(result.stdout).not.toContain("DRY-MARK");
    });
});

describe("+run-filters @e2e (meta filter)", () => {
    it("`-m/--meta` filters tasks by their meta configuration (CEL)", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        fast: {
                            exec: 'echo "FAST-RAN"',
                            meta: { tier: "fast" },
                        },
                        slow: {
                            exec: 'echo "SLOW-RAN"',
                            meta: { tier: "slow" },
                        },
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "fast", "slow", "-m", 'tier == "fast"'],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("FAST-RAN");
        expect(result.stdout).not.toContain("SLOW-RAN");
    });
});

describe("+run-filters @perf (max concurrency)", () => {
    it("`-c/--max-concurrency` still runs every matched task", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                a: { name: "a", tasks: { build: "echo a" } },
                b: { name: "b", tasks: { build: "echo b" } },
                c: { name: "c", tasks: { build: "echo c" } },
            },
        });

        const result = await runOmni(["run", "build", "-c", "1"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Successfully executed 3 tasks");
    });
});

describe("+run-filters @e2e (arg injection)", () => {
    it("`-a/--arg KEY=VALUE` injects args into the invoked command", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { greet: 'echo "{{ args.greeting }}"' },
                },
            },
        });

        const result = await runOmni(
            ["run", "greet", "-a", "greeting=hello-arg"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("hello-arg");
    });
});

describe("+run-filters @e2e (ui mode)", () => {
    it("`-u stream` runs and streams task output", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "UI-MARK"' } },
            },
        });

        const result = await runOmni(["run", "build", "-u", "stream"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("UI-MARK");
    });

    it("`-u tui` is auto-downgraded to stream when stdout is not a TTY", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "UI-MARK"' } },
            },
        });

        // Captured (non-TTY) stdout forces the Stream UI; the run still succeeds
        // and produces the task's output instead of hanging on a TUI.
        const result = await runOmni(["run", "build", "-u", "tui"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("UI-MARK");
    });
});

describe("+run-filters @output (result file)", () => {
    it("`--result` writes results, inferring the format from the extension", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const result = await runOmni(
            ["run", "build", "--result", "results.json"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("results.json")).toBe(true);
        const parsed = JSON.parse(ws.read("results.json"));
        expect(Array.isArray(parsed)).toBe(true);
        expect(parsed.length).toBeGreaterThanOrEqual(1);
    });

    it("`--result-format` overrides the inferred format", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const result = await runOmni(
            [
                "run",
                "build",
                "--result",
                "results.dat",
                "--result-format",
                "yaml",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("results.dat")).toBe(true);
        // YAML sequence syntax, despite the `.dat` extension.
        expect(ws.read("results.dat")).toContain("- status: completed");
    });

    it("`--result` with an unknown extension and no `--result-format` errors", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const result = await runOmni(
            ["run", "build", "--result", "results.xyz"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("unsupported extension");
    });
});

describe("+run-filters @e2e (retry overrides)", () => {
    function flakySpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        flaky: {
                            exec: "false",
                            max_retries: 3,
                            retry_interval: "5ms",
                        },
                    },
                },
            },
        };
    }

    it("`-r/--retry` overrides the task's `max_retries`", async () => {
        const ws = makeWorkspace(flakySpec());

        const result = await runOmni(
            ["run", "flaky", "-r", "0", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // `-r 0` overrides `max_retries: 3`, so there are no retry attempts.
        expect(result.stdout).not.toContain("retrying...");
    });

    it("`--retry-interval` rejects a non-duration value", async () => {
        const ws = makeWorkspace(flakySpec());

        const result = await runOmni(
            ["run", "flaky", "--retry-interval", "notaduration"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining("invalid value");
    });
});

describe("+run-filters @output (execution summary)", () => {
    it("reports success / errored / skipped counts", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        ok: "echo ok",
                        bad: "false",
                        dependent: { exec: "echo dep", dependencies: ["bad"] },
                    },
                },
            },
        });

        const result = await runOmni(["run", "ok", "bad", "dependent"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toOutputContaining("Successfully executed 1 tasks");
        expect(result).toOutputContaining("Failed to execute 1 tasks");
        expect(result).toOutputContaining("Skipped 1 tasks");
    });

    it("counts results served from the cache", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toOutputContaining("0 results from cache");

        const second = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("1 results from cache");
    });
});
