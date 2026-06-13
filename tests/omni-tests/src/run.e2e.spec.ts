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
