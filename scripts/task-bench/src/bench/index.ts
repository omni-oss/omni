import { readFileSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { cpus, tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { execa } from "execa";
import { HarnessConfigSchema, type Tool } from "../config";
import { buildGraph, taskNames } from "../graph";
import { getAdapter, resolveToolVersions, type ToolContext } from "../tools";
import { computeStats, type Stats } from "./stats";

export interface RunSample {
    durationMs: number;
    exitCode: number;
    stdout: string;
    stderr: string;
    /** Number of tasks that actually executed (0 == a full cache hit). */
    executed: number;
    ok: boolean;
}

export interface ScenarioResult {
    runs: number;
    failures: number;
    stats: Stats;
    /** Median number of tasks that actually executed across the runs. */
    executedMedian: number;
}

export interface ToolResult {
    tool: Tool;
    task: string;
    /** Size of the executed task graph (tasks run on a cold, uncached run). */
    taskGraphSize: number;
    cold: ScenarioResult;
    warm: ScenarioResult;
    error?: string;
}

export interface BenchmarkResult {
    rootDir: string;
    task: string;
    projects: number;
    tasksPerProject: number;
    concurrency: number;
    daemon: boolean;
    /** Resolved version of each benchmarked tool (detected for omni). */
    versions: Record<string, string | null>;
    generatedAt: string;
    tools: ToolResult[];
}

export type BenchEvent =
    | { kind: "tool-start"; tool: Tool }
    | {
          kind: "scenario";
          tool: Tool;
          scenario: "cold" | "warm";
          run: number;
          total: number;
          sample: RunSample;
      }
    | { kind: "tool-error"; tool: Tool; error: string }
    | {
          kind: "tool-unsuccessful";
          tool: Tool;
          sample: RunSample;
      };

export interface RunBenchmarkOptions {
    tools?: Tool[] | undefined;
    task?: string | undefined;
    coldRuns?: number | undefined;
    warmRuns?: number | undefined;
    concurrency?: number | undefined;
    /** Allow each tool's persistent daemon (default true). */
    daemon?: boolean | undefined;
    onEvent?: ((event: BenchEvent) => void) | undefined;
}

function median(values: number[]): number {
    if (values.length === 0) return 0;
    const sorted = [...values].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2
        : (sorted[mid] ?? 0);
}

function countLines(path: string): number {
    try {
        const text = readFileSync(path, "utf8");
        if (text.length === 0) return 0;
        // Each executed task appends exactly one newline-terminated line.
        let count = 0;
        for (let i = 0; i < text.length; i++) {
            if (text[i] === "\n") count++;
        }
        return count;
    } catch {
        return 0;
    }
}

async function timeRun(
    file: string,
    args: string[],
    rootDir: string,
    execLog: string,
    extraEnv: Record<string, string>,
): Promise<RunSample> {
    // Reset the execution marker so we can count real executions for this run.
    writeFileSync(execLog, "");
    const start = performance.now();
    const result = await execa(file, args, {
        cwd: rootDir,
        reject: false,
        env: {
            FORCE_COLOR: "0",
            TURBO_TELEMETRY_DISABLED: "1",
            DO_NOT_TRACK: "1",
            NX_TUI: "false",
            TASK_BENCH_EXEC_LOG: execLog,
            ...extraEnv,
        },
    });

    const durationMs = performance.now() - start;
    const stdout = typeof result.stdout === "string" ? result.stdout : "";
    const stderr = typeof result.stderr === "string" ? result.stderr : "";
    return {
        durationMs,
        exitCode: result.exitCode ?? -1,
        stdout,
        stderr,
        executed: countLines(execLog),
        ok: result.exitCode === 0,
    };
}

function scenarioFromSamples(samples: RunSample[]): ScenarioResult {
    return {
        runs: samples.length,
        failures: samples.filter((s) => !s.ok).length,
        stats: computeStats(samples.map((s) => s.durationMs)),
        executedMedian: median(samples.map((s) => s.executed)),
    };
}

/**
 * Benchmark the enabled tools against an already-generated workspace.
 *
 * For each tool we measure two scenarios:
 *   - cold: caches + outputs wiped before every run (discovery + full exec + cache write)
 *   - warm: caches primed, so ideally every task is a cache hit
 *           (isolates discovery + cache-restore overhead)
 *
 * A tool-agnostic execution counter (see the generated task runner) records how
 * many tasks actually ran, so warm-run cache effectiveness is *verified* rather
 * than assumed. Concurrency is pinned identically across all runners.
 */
export async function runBenchmark(
    rootDir: string,
    options: RunBenchmarkOptions = {},
): Promise<BenchmarkResult> {
    const raw = JSON.parse(
        await readFile(join(rootDir, "bench.config.json"), "utf8"),
    );
    const config = HarnessConfigSchema.parse(raw);
    const projects = buildGraph(config);
    const tasks = taskNames(config);

    const tools = options.tools ?? config.tools;
    const task = options.task ?? tasks[tasks.length - 1] ?? "t0";
    const coldRuns = options.coldRuns ?? 3;
    const warmRuns = options.warmRuns ?? 5;
    const concurrency = options.concurrency ?? Math.max(1, cpus().length);
    const daemon = options.daemon ?? true;
    const emit = options.onEvent ?? (() => {});
    const execLog = join(
        tmpdir(),
        `task-bench-exec-${process.pid}-${Date.now()}.log`,
    );

    const ctx: ToolContext = {
        rootDir,
        projectDirs: projects.map((p) => p.dir),
        concurrency,
        daemon,
    };

    // Resolve (and re-validate) the version of each tool actually used.
    const versionMap = await resolveToolVersions(config, rootDir, tools);
    const versions: Record<string, string | null> = {};
    for (const [tool, version] of versionMap) versions[tool] = version;

    const results: ToolResult[] = [];

    for (const tool of tools) {
        emit({ kind: "tool-start", tool });
        const adapter = getAdapter(tool);
        const invocation = adapter.run(task, ctx);
        const env = adapter.env(ctx);

        try {
            // In no-daemon mode make sure no stale daemon lingers first.
            if (!daemon && adapter.hasDaemon) {
                await adapter.stopDaemon(ctx);
            }

            // Cold scenario: a fresh start each run. Caches + outputs are wiped,
            // and (in daemon mode) the daemon is torn down so cold includes its
            // startup cost. Warm runs below deliberately keep the daemon alive.
            const coldSamples: RunSample[] = [];
            for (let run = 1; run <= coldRuns; run++) {
                await adapter.clearCaches(ctx);
                if (daemon && adapter.hasDaemon) {
                    await adapter.stopDaemon(ctx);
                }
                const sample = await timeRun(
                    invocation.file,
                    invocation.args,
                    rootDir,
                    execLog,
                    env,
                );
                coldSamples.push(sample);
                emit({
                    kind: "scenario",
                    tool,
                    scenario: "cold",
                    run,
                    total: coldRuns,
                    sample,
                });
                if (sample.exitCode !== 0) {
                    emit({
                        kind: "tool-unsuccessful",
                        tool,
                        sample,
                    });
                }
            }

            // Warm scenario: prime once (unmeasured, also warms the daemon),
            // then measure while caches + daemon stay hot.
            await timeRun(
                invocation.file,
                invocation.args,
                rootDir,
                execLog,
                env,
            );
            const warmSamples: RunSample[] = [];
            for (let run = 1; run <= warmRuns; run++) {
                const sample = await timeRun(
                    invocation.file,
                    invocation.args,
                    rootDir,
                    execLog,
                    env,
                );
                warmSamples.push(sample);
                emit({
                    kind: "scenario",
                    tool,
                    scenario: "warm",
                    run,
                    total: warmRuns,
                    sample,
                });
                if (sample.exitCode !== 0) {
                    emit({
                        kind: "tool-unsuccessful",
                        tool,
                        sample,
                    });
                }
            }

            const taskGraphSize = Math.max(
                0,
                ...coldSamples.map((s) => s.executed),
                ...warmSamples.map((s) => s.executed),
            );

            results.push({
                tool,
                task,
                taskGraphSize,
                cold: scenarioFromSamples(coldSamples),
                warm: scenarioFromSamples(warmSamples),
            });
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            emit({ kind: "tool-error", tool, error: message });
            results.push({
                tool,
                task,
                taskGraphSize: 0,
                cold: scenarioFromSamples([]),
                warm: scenarioFromSamples([]),
                error: message,
            });
        } finally {
            // Always clean up the daemon so it does not leak between tools or
            // outlive the benchmark.
            await adapter.stopDaemon(ctx);
        }
    }

    return {
        rootDir,
        task,
        projects: config.projects,
        tasksPerProject: config.tasksPerProject,
        concurrency,
        daemon,
        versions,
        generatedAt: new Date().toISOString(),
        tools: results,
    };
}
