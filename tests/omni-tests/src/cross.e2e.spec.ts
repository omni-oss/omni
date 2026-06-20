/**
 * Cross-cutting regression tests that don't belong to a single command.
 *
 * Notes on stream behavior (verified against the binary):
 *   - Command payloads (project lists, env values, generated content) and the
 *     `log` facade output (INFO/WARN/ERROR) all go to *stdout* by default.
 *   - clap argument/usage errors and hard `eyre` failure reports go to *stderr*.
 * So "stdout vs stderr separation" here means: data on stdout, argument errors
 * on stderr. (Routing logs to stderr is opt-in via `--stderr-log-level`, covered by
 * the +global suite.)
 */

import { describe, expect, it } from "vitest";
import {
    type FileContent,
    lines,
    makeWorkspace,
    runOmni,
    singleProjectSpec,
    type WorkspaceSpec,
} from "@/harness";

describe("+cross @output (stream separation)", () => {
    it("writes command data to stdout and argument errors to stderr", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                alpha: { name: "alpha", tasks: { build: "true" } },
                beta: { name: "beta", tasks: { build: "true" } },
            },
        });

        // A data command: payload on stdout, nothing on stderr.
        const data = await runOmni(["project", "list", "-r"], { cwd: ws.cwd });
        expect(data).toHaveSucceeded();
        expect([...lines(data.out)].sort()).toEqual(["alpha", "beta"]);
        expect(data.err).toBe("");

        // A usage error: message on stderr, nothing on stdout.
        const usage = await runOmni(["project", "bogus-subcommand"], {
            cwd: ws.cwd,
        });
        expect(usage).toHaveExitCode(2);
        expect(usage.stdout).toBe("");
        expect(usage).toHaveStderrContaining("unrecognized subcommand");
    });
});

describe("+cross @exitcode (exit codes)", () => {
    function tasksWorkspace() {
        return makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { ok: "true", bad: "false" },
                },
            },
        });
    }

    it("exits 0 on success", async () => {
        const ws = tasksWorkspace();

        const result = await runOmni(["run", "ok"], { cwd: ws.cwd });

        expect(result).toHaveExitCode(0);
    });

    it("exits 1 when a task fails", async () => {
        const ws = tasksWorkspace();

        const result = await runOmni(["run", "bad"], { cwd: ws.cwd });

        expect(result).toHaveExitCode(1);
    });

    it("exits with a non-zero usage code on argument errors", async () => {
        const result = await runOmni(["definitely-not-a-subcommand"]);

        // clap reserves exit code 2 for usage errors.
        expect(result).toHaveExitCode(2);
    });
});

describe("+cross @config (root discovery)", () => {
    it("finds the workspace root from a nested subdirectory", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: { app: { name: "app", tasks: { build: "true" } } },
            files: { "app/deep/deeper/.keep": "" },
        });

        // Running several levels below the root still resolves the workspace.
        const result = await runOmni(["project", "list", "-r"], {
            cwd: ws.path("app", "deep", "deeper"),
        });

        expect(result).toHaveSucceeded();
        expect(lines(result.out)).toContain("app");
    });
});

describe("+cross @cache @perf (concurrent cache access)", () => {
    // Regression for the cache-layer race: when several runs cache the same
    // task digest at once, `HybridTaskExecutionCacheStore::cache_many` used to
    // `remove_dir_all` then recreate the shared per-digest directory in place,
    // so one process deleted files another was mid-write to. It now builds each
    // entry in a private staging dir and atomically renames it into place (and
    // holds a shared prune lock), so concurrent publishers never corrupt the
    // cache. See crates/omni_cache/src/cache/impls/hybrid.rs.
    it("keeps the shared cache intact under concurrent runs", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        // Several runs hammer the same `<root>/.omni/cache` at once.
        const concurrent = await Promise.all(
            Array.from({ length: 5 }, () =>
                runOmni(["run", "build"], { cwd: ws.cwd }),
            ),
        );
        for (const result of concurrent) {
            expect(result).toHaveExitCode(0);
        }

        // A subsequent run still reads a valid entry - the cache wasn't corrupted.
        const replay = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(replay).toHaveSucceeded();
        expect(replay).toOutputContaining("from cache");
    });
});

describe("+cross @perf (large workspace)", () => {
    it("lists a large workspace within a sane time budget", async () => {
        const PROJECT_COUNT = 200;
        const projects: Record<string, FileContent> = {};
        for (let i = 0; i < PROJECT_COUNT; i++) {
            const name = `proj-${String(i).padStart(3, "0")}`;
            projects[name] = { name, tasks: { build: "true" } };
        }
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects,
        });

        const start = Date.now();
        const result = await runOmni(["project", "list", "-r"], {
            cwd: ws.cwd,
            timeout: 60_000,
        });
        const elapsedMs = Date.now() - start;

        expect(result).toHaveSucceeded();
        expect(lines(result.out)).toHaveLength(PROJECT_COUNT);
        // Generous ceiling; this guards against pathological blow-ups, not perf.
        expect(elapsedMs).toBeLessThan(30_000);
    });
});

describe("+cross @e2e @exitcode (empty workspace)", () => {
    // A valid workspace with zero discovered projects must degrade gracefully:
    // listing yields nothing rather than erroring, and a `run` cleanly reports
    // that there is nothing to execute.
    it("lists nothing and fails `run` with no task to execute", async () => {
        const ws = makeWorkspace({ workspace: { projects: ["**"] } });

        const list = await runOmni(["project", "list", "-r"], {
            cwd: ws.cwd,
        });
        expect(list).toHaveSucceeded();
        expect(list.stdout).toBe("");

        const jsonList = await runOmni(
            ["project", "list", "-r", "-f", "json"],
            { cwd: ws.cwd },
        );
        expect(jsonList).toHaveSucceeded();
        expect(JSON.parse(jsonList.stdout)).toEqual([]);

        const run = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(run).toHaveFailed();
        expect(run).toHaveStderrContaining("no task to execute");
    });
});

describe("+cross @output (global flags + run filters)", () => {
    // The global `-l/--stdout-logs-level` flag sits on the top-level `CliArgs`
    // and must precede the subcommand. With `-l off` the `log` facade output
    // (the "Successfully executed ..." summary is a `log::info!`) is silenced,
    // but the task's own echoed payload still streams to stdout. Combining the
    // global flag with `run`'s `-p`/`-m` filters must not leak anything to
    // stderr. See crates/omni_cli_core/src/commands/mod.rs (CliArgs) and
    // crates/omni_cli_core/src/commands/utils.rs (report_execution_results).
    it("`-l off` silences logs while filtered run data stays on stdout only", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "DATA-MARKER"',
                            meta: { tier: "fast" },
                        },
                    },
                },
                other: {
                    name: "other",
                    tasks: { build: 'echo "OTHER-MARKER"' },
                },
            },
        });

        const result = await runOmni(
            ["-l", "off", "run", "build", "-p", "app", "-m", 'tier == "fast"'],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // The task's echoed payload survives on stdout...
        expect(result).toOutputContaining("DATA-MARKER");
        // ...the `-p app` filter excluded the other project...
        expect(result.stdout).not.toContain("OTHER-MARKER");
        // ...and `-l off` silenced the INFO-level execution summary.
        expect(result.stdout).not.toContain("Successfully executed");
        // Nothing - neither logs nor data - leaked to stderr.
        expect(result.err).toBe("");
    });
});

describe("+cross @cache (run + exec coexistence)", () => {
    // `omni exec` reuses `run`'s flattened `RunArgs` but builds an ad-hoc
    // command `Call` (see crates/omni_cli_core/src/commands/exec.rs), so it
    // never consults the run task cache. A cached `run` task and a later `exec`
    // of a different command must each succeed without interfering.
    it("a cached `run` task and a later `exec` both succeed independently", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        // Prime the run cache.
        const first = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();
        expect(first).toOutputContaining("0 results from cache");

        // The second run is served from the cache.
        const cached = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(cached).toHaveSucceeded();
        expect(cached).toOutputContaining("1 results from cache");

        // exec runs an entirely different ad-hoc command, bypassing the cache.
        const exec = await runOmni(["exec", "--", "echo", "EXEC-MARKER"], {
            cwd: ws.cwd,
        });
        expect(exec).toHaveSucceeded();
        expect(exec).toOutputContaining("EXEC-MARKER");
        // exec did not replay the cached run output.
        expect(exec.stdout).not.toContain("build app");

        // The run cache is still intact after the exec.
        const replay = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(replay).toHaveSucceeded();
        expect(replay).toOutputContaining("1 results from cache");
    });
});

describe("+cross @cli (task selector glob matching)", () => {
    // Task selectors are compiled into a `GlobSet` (see
    // crates/omni_execution_plan/src/filter.rs: DefaultTaskFilter), so `run`
    // matches task names as glob patterns rather than literal strings. A
    // wildcard like `bui*` therefore expands to `build`, an exact name matches
    // itself, and a pattern matching nothing yields the executor's
    // "no task to execute" error.
    it("matches task selectors as glob patterns, not literal strings", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: 'echo "BUILT"' } },
            },
        });

        // A glob wildcard expands to the matching task and runs it.
        const glob = await runOmni(["run", "bui*"], { cwd: ws.cwd });
        expect(glob).toHaveSucceeded();
        expect(glob).toOutputContaining("BUILT");

        // An exact name matches itself.
        const literal = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(literal).toHaveSucceeded();
        expect(literal).toOutputContaining("BUILT");

        // A pattern that matches no task name fails with the executor error.
        const unmatched = await runOmni(["run", "nope*"], { cwd: ws.cwd });
        expect(unmatched).toHaveFailed();
        expect(unmatched).toHaveStderrContaining("no task to execute");
        expect(unmatched.stdout).not.toContain("BUILT");
    });
});

describe("+cross @output (unicode round-trip)", () => {
    // Spaces and non-ASCII characters in project names and task output must
    // survive the full spawn/capture round-trip without mojibake or trunctation.
    it("preserves spaces and unicode in project names and task output", async () => {
        const projectName = "héllo wörld 🌍";
        const taskOutput = "héllo wörld 🌍";
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: projectName,
                    tasks: { greet: `echo "${taskOutput}"` },
                },
            },
        });

        // The unicode project name round-trips through `project list`.
        const list = await runOmni(["project", "list", "-r"], { cwd: ws.cwd });
        expect(list).toHaveSucceeded();
        expect(lines(list.out)).toContain(projectName);

        // The unicode task output round-trips through stdout intact.
        const run = await runOmni(["run", "greet"], { cwd: ws.cwd });
        expect(run).toHaveSucceeded();
        expect(run).toOutputContaining(taskOutput);
    });
});

describe("+cross @cli (many args + long values)", () => {
    // Repeated `-a/--arg` flags accumulate without dropping entries, and a very
    // long value is injected verbatim with no truncation. The task echoes every
    // injected key so each one is observable on stdout.
    it("injects many `-a` flags and a long value without truncation", async () => {
        const KEY_COUNT = 12;
        const keys = Array.from({ length: KEY_COUNT }, (_, i) => `key_${i}`);
        // A distinctive, hard-to-truncate-silently value per key.
        const valueFor = (i: number) => `value-${i}-${"x".repeat(i)}`;
        // One value far longer than any plausible buffer boundary.
        const longValue = `L${"o".repeat(1024)}NG`;

        const template = [
            ...keys.map((k) => `[{{ args.${k} }}]`),
            `[{{ args.long }}]`,
        ].join(" ");

        const spec: WorkspaceSpec = {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: { greet: `echo "${template}"` },
                },
            },
        };
        const ws = makeWorkspace(spec);

        const argFlags = keys.flatMap((k, i) => ["-a", `${k}=${valueFor(i)}`]);
        // `-l off` silences interleaved INFO logs so the streamed echo output
        // stays contiguous and the long value can be matched in one piece.
        const result = await runOmni(
            [
                "-l",
                "off",
                "run",
                "greet",
                ...argFlags,
                "-a",
                `long=${longValue}`,
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Every short arg is present, wrapped in its marker.
        for (let i = 0; i < KEY_COUNT; i++) {
            expect(result).toOutputContaining(`[${valueFor(i)}]`);
        }
        // The long value survives in full.
        expect(result).toOutputContaining(`[${longValue}]`);
    });
});
