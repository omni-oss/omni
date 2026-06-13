/**
 * `omni run` e2e tests: task matching, dependency ordering, failure handling,
 * dependency syntaxes (`^task`, `project#task`), siblings (`with`), gating
 * (`enabled`/`if`), retries, and persistent-task semantics.
 *
 * Pinned to `crates/omni_cli_core/src/commands/run.rs`, the execution-plan
 * builder in `crates/omni_execution_plan/`, and the task graph in
 * `crates/omni_core/src/task_execution_graph.rs`. Tasks use `echo` so their
 * recorded output is observable; failing tasks use `false`.
 */

import { describe, expect, it } from "vitest";
import {
    dependencyChainSpec,
    makeWorkspace,
    runOmni,
    singleProjectSpec,
    type WorkspaceSpec,
} from "@/harness";

describe("+run @e2e (basic execution)", () => {
    it("runs a matching task and exits 0", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["run", "build"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("build app");
    });

    it("runs every positional task when several are given", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["run", "build", "test"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("build app");
        expect(result).toOutputContaining("test app");
        expect(result).toOutputContaining("Successfully executed 2 tasks");
    });
});

describe("+run @exitcode (failure handling)", () => {
    it("a failing task yields a non-zero exit", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: { app: { name: "app", tasks: { bad: "false" } } },
        });

        const result = await runOmni(["run", "bad"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
    });

    it("a task matching nothing reports the unmatched call and fails", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["run", "does-not-exist"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no task to execute");
    });
});

describe("+run @e2e (dependencies)", () => {
    it("runs dependencies before the dependent task", async () => {
        const ws = makeWorkspace(dependencyChainSpec());

        const result = await runOmni(["run", "run", "-p", "project-1"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        const out = result.stdout;
        // `project-1#run` depends on `project-2#list` and the local `build`.
        expect(out.indexOf("list project-2")).toBeGreaterThanOrEqual(0);
        expect(out.indexOf("build project-1")).toBeGreaterThanOrEqual(0);
        expect(out.indexOf("list project-2")).toBeLessThan(
            out.indexOf("run project-1"),
        );
        expect(out.indexOf("build project-1")).toBeLessThan(
            out.indexOf("run project-1"),
        );
    });

    it("`-i/--ignore-dependencies` skips dependency tasks", async () => {
        const ws = makeWorkspace(dependencyChainSpec());

        const result = await runOmni(["run", "run", "-p", "project-1", "-i"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("run project-1");
        // Dependencies were skipped.
        expect(result.stdout).not.toContain("list project-2");
        expect(result.stdout).not.toContain("build project-1");
    });

    it("`-w/--with-dependents` also runs dependents of matched tasks", async () => {
        // `app#build` depends on its upstream (`lib#build`), so `lib#build` is a
        // dependency of `app#build`; pulling dependents of `lib#build` brings in
        // `app#build`.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    dependencies: ["lib"],
                    tasks: {
                        build: {
                            exec: 'echo "APP-BUILD"',
                            dependencies: ["^build"],
                        },
                    },
                },
                lib: { name: "lib", tasks: { build: 'echo "LIB-BUILD"' } },
            },
        });

        const withDependents = await runOmni(
            ["run", "build", "-p", "lib", "-w"],
            { cwd: ws.cwd },
        );
        expect(withDependents).toHaveSucceeded();
        expect(withDependents).toOutputContaining("LIB-BUILD");
        expect(withDependents).toOutputContaining("APP-BUILD");

        // Without `-w`, only the matched `lib#build` runs.
        const plain = await runOmni(["run", "build", "-p", "lib"], {
            cwd: ws.cwd,
        });
        expect(plain).toHaveSucceeded();
        expect(plain.stdout).not.toContain("APP-BUILD");
    });

    it("`--ignore-deps` and `--with-dependents` are mutually exclusive", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(
            ["run", "build", "--ignore-deps", "--with-dependents"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(2);
    });
});

describe("+run @e2e (dependency syntaxes)", () => {
    // `^build` resolves to the `build` task of dependency projects; an explicit
    // `lib#compile` targets a specific project task. Both must run before the
    // dependent `app#test`.
    function crossProjectDepsSpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    dependencies: ["lib"],
                    tasks: {
                        test: {
                            exec: 'echo "APP-TEST"',
                            dependencies: ["^build", "lib#compile"],
                        },
                    },
                },
                lib: {
                    name: "lib",
                    tasks: {
                        build: 'echo "LIB-BUILD"',
                        compile: 'echo "LIB-COMPILE"',
                    },
                },
            },
        };
    }

    it("resolves the `^task` upstream-dependency syntax", async () => {
        const ws = makeWorkspace(crossProjectDepsSpec());

        const result = await runOmni(["run", "test", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        // The upstream `lib#build` ran via `^build`, before `app#test`.
        expect(result).toOutputContaining("LIB-BUILD");
        expect(result.stdout.indexOf("LIB-BUILD")).toBeLessThan(
            result.stdout.indexOf("APP-TEST"),
        );
    });

    it("resolves the explicit `project#task` dependency syntax", async () => {
        const ws = makeWorkspace(crossProjectDepsSpec());

        const result = await runOmni(["run", "test", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("LIB-COMPILE");
        expect(result.stdout.indexOf("LIB-COMPILE")).toBeLessThan(
            result.stdout.indexOf("APP-TEST"),
        );
    });

    it("runs `with` siblings alongside the task", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        sib: { exec: 'echo "MAIN-SIB"', with: ["withsib"] },
                        withsib: 'echo "WITH-SIB"',
                    },
                },
            },
        });

        const result = await runOmni(["run", "sib", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("MAIN-SIB");
        expect(result).toOutputContaining("WITH-SIB");
    });
});

describe("+run @e2e @exitcode (cycles & gating)", () => {
    it("detects and reports a cyclic dependency graph instead of hanging", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        x: { exec: "echo x", dependencies: ["y"] },
                        y: { exec: "echo y", dependencies: ["x"] },
                    },
                },
            },
        });

        const result = await runOmni(["run", "x", "-p", "app"], {
            cwd: ws.cwd,
            timeout: 15_000,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("cycle detected");
    });

    it("`enabled`/`if` gates task execution", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        gated: { exec: 'echo "SHOULD-NOT-RUN"', if: "false" },
                        live: { exec: 'echo "SHOULD-RUN"', if: "true" },
                    },
                },
            },
        });

        const gated = await runOmni(["run", "gated", "-p", "app"], {
            cwd: ws.cwd,
        });
        expect(gated).toHaveSucceeded();
        expect(gated).toOutputContaining("Skipping disabled task 'app#gated'");
        expect(gated.stdout).not.toContain("SHOULD-NOT-RUN");

        const live = await runOmni(["run", "live", "-p", "app"], {
            cwd: ws.cwd,
        });
        expect(live).toHaveSucceeded();
        expect(live).toOutputContaining("SHOULD-RUN");
    });
});

describe("+run @e2e (on-failure)", () => {
    function failingDependentSpec(): WorkspaceSpec {
        return {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        base: "false",
                        dependent: {
                            exec: 'echo "DEPENDENT-RAN"',
                            dependencies: ["base"],
                        },
                    },
                },
            },
        };
    }

    it("skips dependents of a failed task by default (skip-dependents)", async () => {
        const ws = makeWorkspace(failingDependentSpec());

        const result = await runOmni(["run", "dependent", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toOutputContaining(
            "Skipping task 'app#dependent' due to failed dependency 'app#base'",
        );
        expect(result.stdout).not.toContain("DEPENDENT-RAN");
    });

    it("`-o continue` runs dependents even when a dependency failed", async () => {
        const ws = makeWorkspace(failingDependentSpec());

        const result = await runOmni(
            ["run", "dependent", "-p", "app", "-o", "continue"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // `continue` lets the dependent run despite the failed dependency.
        expect(result).toOutputContaining("DEPENDENT-RAN");
    });
});

describe("+run @e2e (retries & persistence)", () => {
    it("`max_retries` retries a failing task before giving up", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        flaky: {
                            exec: "false",
                            max_retries: 2,
                            retry_interval: "5ms",
                        },
                    },
                },
            },
        });

        // `--no-cache` keeps a prior cached failure from short-circuiting retries.
        const result = await runOmni(
            ["run", "flaky", "-p", "app", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // `max_retries: 2` produces two retry attempts before the final failure.
        const attempts = (result.stdout.match(/retrying\.\.\./g) ?? []).length;
        expect(attempts).toBe(2);
    });

    it("a persistent task is never served from the cache", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        serve: { exec: 'echo "PERSIST-RAN"', persistent: true },
                    },
                },
            },
        });

        const first = await runOmni(["run", "serve", "-p", "app"], {
            cwd: ws.cwd,
        });
        expect(first).toHaveSucceeded();
        expect(first).toOutputContaining("PERSIST-RAN");

        // Re-running executes again rather than replaying a cache hit.
        const second = await runOmni(["run", "serve", "-p", "app"], {
            cwd: ws.cwd,
        });
        expect(second).toHaveSucceeded();
        expect(second).toOutputContaining("PERSIST-RAN");
        expect(second.stdout).not.toContain("Cache hit for task");
        expect(second).toOutputContaining("0 results from cache");
    });
});

describe("+run @e2e (filter + dependency combinations)", () => {
    it("`-i` + `-p` runs only the matched task in the matched project", async () => {
        const ws = makeWorkspace(dependencyChainSpec());

        const result = await runOmni(["run", "run", "-p", "project-1", "-i"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("run project-1");
        // No dependencies run: neither the local `build` nor `project-2#list`.
        expect(result.stdout).not.toContain("build project-1");
        expect(result.stdout).not.toContain("list project-2");
        expect(result).toOutputContaining("Successfully executed 1 tasks");
    });

    it("`-w` + `-m` filters roots by meta, then pulls dependents in by task name", async () => {
        // The meta filter selects the matched *roots* (`core#build`), not the
        // dependents: `-w` pulls in every dependent sharing the queried task
        // name regardless of its own meta. `legacy#build` is excluded because
        // it is neither a meta-matched root nor a dependent of one.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                core: {
                    name: "core",
                    tasks: {
                        build: {
                            exec: 'echo "CORE-BUILD"',
                            meta: { tier: "fast" },
                        },
                    },
                },
                app: {
                    name: "app",
                    dependencies: ["core"],
                    tasks: {
                        build: {
                            exec: 'echo "APP-BUILD"',
                            dependencies: ["^build"],
                            meta: { tier: "slow" },
                        },
                    },
                },
                legacy: {
                    name: "legacy",
                    tasks: {
                        build: {
                            exec: 'echo "LEGACY-BUILD"',
                            meta: { tier: "slow" },
                        },
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "build", "-w", "-m", 'tier == "fast"'],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("CORE-BUILD");
        // `app#build` is a `slow` dependent of the `fast` root, pulled in by `-w`.
        expect(result).toOutputContaining("APP-BUILD");
        // `legacy#build` is `slow` and not a dependent, so the meta filter drops it.
        expect(result.stdout).not.toContain("LEGACY-BUILD");
    });

    it("multiple positional tasks + `-p` glob run the full matched set", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "app-one": {
                    name: "app-one",
                    tasks: {
                        build: 'echo "ONE-BUILD"',
                        test: 'echo "ONE-TEST"',
                    },
                },
                "app-two": {
                    name: "app-two",
                    tasks: {
                        build: 'echo "TWO-BUILD"',
                        test: 'echo "TWO-TEST"',
                    },
                },
                other: {
                    name: "other",
                    tasks: { build: 'echo "OTHER-BUILD"' },
                },
            },
        });

        const result = await runOmni(["run", "build", "test", "-p", "app-*"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("ONE-BUILD");
        expect(result).toOutputContaining("ONE-TEST");
        expect(result).toOutputContaining("TWO-BUILD");
        expect(result).toOutputContaining("TWO-TEST");
        expect(result.stdout).not.toContain("OTHER-BUILD");
        expect(result).toOutputContaining("Successfully executed 4 tasks");
    });
});

describe("+run @cache (force + cache flag combinations)", () => {
    it("`--force` + `--no-cache` re-executes but does not persist the result", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "BUILD-MARK"' } },
            },
        });

        // Forced + non-persisted: executes fresh and writes nothing to the cache.
        const forced = await runOmni(
            ["run", "build", "--force", "--no-cache"],
            { cwd: ws.cwd },
        );
        expect(forced).toHaveSucceeded();
        expect(forced).toOutputContaining("BUILD-MARK");
        expect(forced).toOutputContaining("0 results from cache");

        // Because nothing was persisted, the next plain run is still a miss.
        const miss = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(miss).toOutputContaining("0 results from cache");

        // Now that the miss above persisted, a third run is a cache hit.
        const hit = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(hit).toOutputContaining("1 results from cache");
    });

    it("`-f` + `-L` re-executes and shows fresh (not replayed) logs", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "BUILD-MARK"' } },
            },
        });

        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const forced = await runOmni(["run", "build", "-f", "-L"], {
            cwd: ws.cwd,
        });
        expect(forced).toHaveSucceeded();
        // Forced re-execution produces fresh output, not a replayed cache hit.
        expect(forced).toOutputContaining("BUILD-MARK");
        expect(forced).toOutputContaining("0 results from cache");
        expect(forced.stdout).not.toContain("Cache hit for task");
    });
});

describe("+run @e2e (on-failure with independent tasks)", () => {
    it("`-o continue` runs the remaining independent tasks after one fails", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        first: 'echo "FIRST-RAN"',
                        bad: "false",
                        last: 'echo "LAST-RAN"',
                    },
                },
            },
        });

        const result = await runOmni(
            ["run", "first", "bad", "last", "-o", "continue"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // The failing task does not stop the independent ones.
        expect(result).toOutputContaining("FIRST-RAN");
        expect(result).toOutputContaining("LAST-RAN");
    });
});

describe("+run @output (dry-run + result file)", () => {
    it("`--dry-run` + `--result` still writes the results file", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "DRY-MARK"' } },
            },
        });

        const result = await runOmni(
            ["run", "build", "--dry-run", "--result", "results.json"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // The task never executes, yet the results file is written.
        expect(result.stdout).not.toContain("DRY-MARK");
        expect(ws.exists("results.json")).toBe(true);
        const parsed = JSON.parse(ws.read("results.json"));
        expect(Array.isArray(parsed)).toBe(true);
        expect(parsed.length).toBeGreaterThanOrEqual(1);
    });
});

describe("+run @e2e (arg-gated execution)", () => {
    it("`-a KEY=VAL` referenced inside an `if` expression gates the task", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        gated: {
                            exec: 'echo "GATED-RAN"',
                            if: '{{ args.flag == "yes" }}',
                        },
                    },
                },
            },
        });

        const enabled = await runOmni(
            ["run", "gated", "-p", "app", "-a", "flag=yes"],
            { cwd: ws.cwd },
        );
        expect(enabled).toHaveSucceeded();
        expect(enabled).toOutputContaining("GATED-RAN");

        const disabled = await runOmni(
            ["run", "gated", "-p", "app", "-a", "flag=no"],
            { cwd: ws.cwd },
        );
        expect(disabled).toHaveSucceeded();
        expect(disabled).toOutputContaining(
            "Skipping disabled task 'app#gated'",
        );
        expect(disabled.stdout).not.toContain("GATED-RAN");
    });
});

describe("+run @e2e (template context)", () => {
    // The per-task template context (default_provider.rs) exposes `env` (resolved
    // workspace/project env vars) and the standard `platform` table
    // (omni_tera/context.rs), so a task `exec` can interpolate both.
    it("resolves workspace env vars via `{{ env.VAR }}`", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".env": "GREETING=hello-from-env\n" },
            projects: {
                app: {
                    name: "app",
                    tasks: { greet: 'echo "<{{ env.GREETING }}>"' },
                },
            },
        });

        const result = await runOmni(["run", "greet", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("<hello-from-env>");
    });

    it("resolves the standard `{{ platform.* }}` context", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { plat: 'echo "OS:[{{ platform.os }}]"' },
                },
            },
        });

        const result = await runOmni(["run", "plat", "-p", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        // The exact OS is host-dependent; assert the template rendered to a
        // non-empty value rather than leaking the raw `{{ ... }}` placeholder.
        expect(result).toMatchOutput(/OS:\[.+\]/);
        expect(result.stdout).not.toContain("{{");
    });
});
