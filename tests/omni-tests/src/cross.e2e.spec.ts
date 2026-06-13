/**
 * Cross-cutting regression tests that don't belong to a single command.
 *
 * Notes on stream behavior (verified against the binary):
 *   - Command payloads (project lists, env values, generated content) and the
 *     `log` facade output (INFO/WARN/ERROR) all go to *stdout* by default.
 *   - clap argument/usage errors and hard `eyre` failure reports go to *stderr*.
 * So "stdout vs stderr separation" here means: data on stdout, argument errors
 * on stderr. (Routing logs to stderr is opt-in via `--stderr-log`, covered by
 * the +global suite.)
 */

import { describe, expect, it } from "vitest";
import {
    type FileContent,
    lines,
    makeWorkspace,
    runOmni,
    singleProjectSpec,
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
