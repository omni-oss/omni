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

describe("+run-filters @cli (arg parsing edge cases)", () => {
    // `parse_key_value` splits on the first `=`, strips one layer of matching
    // surrounding quotes, and errors when `=` is absent. The task echoes the
    // injected value wrapped in markers so an empty value is still observable.
    function echoArgSpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { greet: 'echo "<{{ args.x }}>"' },
                },
            },
        };
    }

    it("splits on the first `=`, keeping the remainder in the value", async () => {
        const ws = makeWorkspace(echoArgSpec());

        const result = await runOmni(["run", "greet", "-a", "x=a=b=c"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("<a=b=c>");
    });

    it("strips one layer of matching surrounding quotes", async () => {
        const ws = makeWorkspace(echoArgSpec());

        const dq = await runOmni(["run", "greet", "-a", 'x="hello world"'], {
            cwd: ws.cwd,
        });
        expect(dq).toHaveSucceeded();
        expect(dq).toOutputContaining("<hello world>");

        const sq = await runOmni(["run", "greet", "-a", "x='hello world'"], {
            cwd: ws.cwd,
        });
        expect(sq).toHaveSucceeded();
        expect(sq).toOutputContaining("<hello world>");
    });

    it("leaves mismatched/unbalanced quotes intact", async () => {
        const ws = makeWorkspace(echoArgSpec());

        // Only a *matching* surrounding pair is stripped; a lone leading quote
        // is preserved verbatim.
        const result = await runOmni(
            ["run", "greet", "-a", "x='only-leading"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("<'only-leading>");
    });

    it("accepts an empty value", async () => {
        const ws = makeWorkspace(echoArgSpec());

        const result = await runOmni(["run", "greet", "-a", "x="], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("<>");
    });

    it("rejects a `-a` value with no `=` as a parse error", async () => {
        const ws = makeWorkspace(echoArgSpec());

        const result = await runOmni(["run", "greet", "-a", "noequals"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining("no `=`");
    });

    it("injects multiple independent args from repeated `-a` flags", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        greet: 'echo "{{ args.first }}+{{ args.second }}"',
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "greet", "-a", "first=one", "-a", "second=two"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("one+two");
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

describe("+run-filters @e2e (combined filters)", () => {
    /** Three projects spread across two directories with distinct build markers. */
    function spreadSpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                "svc/api": {
                    name: "api",
                    tasks: { build: 'echo "BUILD-API"' },
                },
                "svc/web": {
                    name: "web",
                    tasks: { build: 'echo "BUILD-WEB"' },
                },
                "tools/api": {
                    name: "api-tool",
                    tasks: { build: 'echo "BUILD-TOOL"' },
                },
            },
        };
    }

    it("`-p` + `--dir` narrows to projects matching BOTH filters", async () => {
        const ws = makeWorkspace(spreadSpec());

        // `-p api*` alone matches api + api-tool; `--dir svc/**` alone matches
        // api + web. Their intersection is only `api`.
        const result = await runOmni(
            ["run", "build", "-p", "api*", "--dir", "svc/**"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-API");
        expect(result.stdout).not.toContain("BUILD-WEB");
        expect(result.stdout).not.toContain("BUILD-TOOL");
    });

    it("`-p` + `-m` applies the project and meta filters together", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-APP"',
                            meta: { tier: "fast" },
                        },
                    },
                },
                other: {
                    name: "other",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-OTHER"',
                            meta: { tier: "fast" },
                        },
                    },
                },
            },
        });

        // Both projects satisfy the meta filter, but `-p app` narrows to one.
        const result = await runOmni(
            ["run", "build", "-p", "app", "-m", 'tier == "fast"'],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-APP");
        expect(result.stdout).not.toContain("BUILD-OTHER");
    });

    it("unions the matched projects across multiple `-p` and `--dir` globs", async () => {
        const ws = makeWorkspace(spreadSpec());

        const byProject = await runOmni(
            ["run", "build", "-p", "web", "-p", "api-tool"],
            { cwd: ws.cwd },
        );
        expect(byProject).toHaveSucceeded();
        expect(byProject).toOutputContaining("BUILD-WEB");
        expect(byProject).toOutputContaining("BUILD-TOOL");
        expect(byProject.stdout).not.toContain("BUILD-API");

        const byDir = await runOmni(
            ["run", "build", "--dir", "svc/**", "--dir", "tools/**"],
            { cwd: ws.cwd },
        );
        expect(byDir).toHaveSucceeded();
        expect(byDir).toOutputContaining("BUILD-API");
        expect(byDir).toOutputContaining("BUILD-WEB");
        expect(byDir).toOutputContaining("BUILD-TOOL");
    });

    it("a `--dir` glob matching nothing reports the no-task error (non-zero)", async () => {
        const ws = makeWorkspace(distinctBuildsSpec());

        const result = await runOmni(
            ["run", "build", "--dir", "does-not-exist/**"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no task to execute");
    });
});

describe("+run-filters @perf (concurrency + dependencies)", () => {
    it("`-c 1` still honors dependency ordering", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        first: 'echo "STEP-FIRST"',
                        second: {
                            exec: 'echo "STEP-SECOND"',
                            dependencies: ["first"],
                        },
                    },
                },
            },
        });

        const result = await runOmni(["run", "second", "-c", "1"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        // The dependency must complete before its dependent, regardless of the
        // single-slot concurrency limit.
        const firstIdx = result.stdout.indexOf("STEP-FIRST");
        const secondIdx = result.stdout.indexOf("STEP-SECOND");
        expect(firstIdx).toBeGreaterThanOrEqual(0);
        expect(secondIdx).toBeGreaterThan(firstIdx);
    });
});

describe("+run-filters @output (result + dry-run combination)", () => {
    it("`--result` + `--result-format` honor the format even under `--dry-run`", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "DRY-MARK"' } },
            },
        });

        const result = await runOmni(
            [
                "run",
                "build",
                "--dry-run",
                "--result",
                "results.dat",
                "--result-format",
                "yaml",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // The task never executes, but the results file is still written in the
        // requested format regardless of the `.dat` extension.
        expect(result.stdout).not.toContain("DRY-MARK");
        expect(ws.exists("results.dat")).toBe(true);
        expect(ws.read("results.dat")).toContain("status:");
    });
});

describe("+run-filters @output (ui downgrade)", () => {
    it("`-u tui` + non-TTY + `--dry-run` downgrades to stream and prints the plan", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "PLAN-MARK"' } },
            },
        });

        // Captured (non-TTY) stdout forces Stream UI; combined with dry-run the
        // plan is printed rather than hanging on a TUI.
        const result = await runOmni(["run", "build", "-u", "tui", "-d"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Dry run mode enabled");
        expect(result).toOutputContaining("Executing task 'app#build'");
        expect(result.stdout).not.toContain("PLAN-MARK");
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
