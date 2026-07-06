import { readFileSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { execa } from "execa";
import { HarnessConfigSchema, type Tool } from "../config";
import { buildGraph, taskNames } from "../graph";
import {
    getAdapter,
    resolveToolVersions,
    type ToolAdapter,
    type ToolContext,
} from "../tools";
import { BASE_ENV } from "./env";
import { getPlatformInfo, type PlatformInfo } from "./platform-info";
import { createProcessProbe, type ProcessProbe } from "./process-probe";
import { measureRun, type ResourceSample } from "./resource-usage";
import { computeStats, median, type Stats } from "./stats";

export interface RunSample {
    durationMs: number;
    exitCode: number;
    stdout: string;
    stderr: string;
    /** Number of tasks that actually executed (0 == a full cache hit). */
    executed: number;
    ok: boolean;
}

/**
 * Resource usage for a scenario, collected in dedicated passes separate from
 * the timed runs (so sampling can't taint `stats`). All three are summarized
 * with the same median/stddev machinery as durations.
 */
export interface ResourceStats {
    runs: number;
    /** Peak summed RSS (bytes) of the tool + daemon processes. */
    peakRssBytes: Stats;
    /** Total CPU time (user+sys, ms) attributed to the run. */
    cpuTimeMs: Stats;
    /** Average cores used (cpuTimeMs / wallMs). */
    parallelism: Stats;
}

export interface ScenarioResult {
    runs: number;
    failures: number;
    stats: Stats;
    /** Median number of tasks that actually executed across the runs. */
    executedMedian: number;
    /** Resource usage (RSS/CPU), when measured; omitted if resourceRuns is 0. */
    resources?: ResourceStats;
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
    platform: PlatformInfo;
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
    | { kind: "tool-unsuccessful"; tool: Tool; sample: RunSample };

export interface RunBenchmarkOptions {
    tools?: Tool[] | undefined;
    task?: string | undefined;
    coldRuns?: number | undefined;
    warmRuns?: number | undefined;
    /**
     * Dedicated resource-measurement passes per scenario (RSS/CPU). Each pass
     * is a full extra invocation, so cold passes are costly. 0 disables it.
     */
    resourceRuns?: number | undefined;
    concurrency?: number | undefined;
    /** Allow each tool's persistent daemon (default true). */
    daemon?: boolean | undefined;
    onEvent?: ((event: BenchEvent) => void) | undefined;
}

/** A zero-argument runner that executes one measured invocation. */
type RunOnce = () => Promise<RunSample>;

function countLines(path: string): number {
    try {
        const text = readFileSync(path, "utf8");
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
        env: { ...BASE_ENV, TASK_BENCH_EXEC_LOG: execLog, ...extraEnv },
    });
    const durationMs = performance.now() - start;
    return {
        durationMs,
        exitCode: result.exitCode ?? -1,
        stdout: typeof result.stdout === "string" ? result.stdout : "",
        stderr: typeof result.stderr === "string" ? result.stderr : "",
        executed: countLines(execLog),
        ok: result.exitCode === 0,
    };
}

function scenarioFromSamples(
    samples: RunSample[],
    resources?: ResourceStats,
): ScenarioResult {
    return {
        runs: samples.length,
        failures: samples.filter((s) => !s.ok).length,
        stats: computeStats(samples.map((s) => s.durationMs)),
        executedMedian: median(samples.map((s) => s.executed)),
        ...(resources ? { resources } : {}),
    };
}

/** Summarize resource samples, or undefined when none were collected. */
function resourceStatsFrom(
    samples: ResourceSample[],
): ResourceStats | undefined {
    if (samples.length === 0) return undefined;
    return {
        runs: samples.length,
        peakRssBytes: computeStats(samples.map((s) => s.peakRssBytes)),
        cpuTimeMs: computeStats(samples.map((s) => s.cpuTimeMs)),
        parallelism: computeStats(samples.map((s) => s.parallelism)),
    };
}

/** Run one scenario `runs` times, calling `before` (if any) before each run. */
async function runScenario(
    runOnce: RunOnce,
    runs: number,
    onSample: (run: number, sample: RunSample) => void,
    before?: () => Promise<void>,
): Promise<RunSample[]> {
    const samples: RunSample[] = [];
    for (let run = 1; run <= runs; run++) {
        if (before) await before();
        const sample = await runOnce();
        samples.push(sample);
        onSample(run, sample);
    }
    return samples;
}

/** Benchmark a single tool (cold + warm), always cleaning up afterwards. */
async function benchmarkTool(
    adapter: ToolAdapter,
    ctx: ToolContext,
    task: string,
    coldRuns: number,
    warmRuns: number,
    resourceRuns: number,
    execLog: string,
    emit: (event: BenchEvent) => void,
): Promise<ToolResult> {
    const invocation = adapter.run(task, ctx);
    const env = adapter.env(ctx);
    const runOnce: RunOnce = () =>
        timeRun(invocation.file, invocation.args, ctx.rootDir, execLog, env);

    // A single resource-measurement pass. Any persistent daemon is resolved
    // up-front (it is only already up on warm passes); a `resolveDaemonPids`
    // callback lets `measureRun` discover a daemon the invocation starts for
    // itself (cold passes) so the whole process tree can be sampled.
    const daemonResolver =
        ctx.daemon && adapter.daemonPids
            ? () =>
                  adapter.daemonPids?.(ctx).catch(() => []) ??
                  Promise.resolve([])
            : undefined;
    const measureOnce = async (
        probe: ProcessProbe,
    ): Promise<ResourceSample> => {
        const daemonPids = daemonResolver ? await daemonResolver() : [];
        return measureRun(
            probe,
            invocation.file,
            invocation.args,
            ctx.rootDir,
            execLog,
            env,
            daemonPids,
            daemonResolver,
        );
    };
    const runResource = async (
        probe: ProcessProbe,
        runs: number,
        before?: () => Promise<void>,
    ): Promise<ResourceSample[]> => {
        const samples: ResourceSample[] = [];
        for (let i = 0; i < runs; i++) {
            if (before) await before();
            samples.push(await measureOnce(probe));
        }
        return samples;
    };

    // Cold reset applied before every cold run/pass: wipe caches + outputs and
    // (in daemon mode) tear the daemon down so cold includes its startup cost.
    const coldBefore = async (): Promise<void> => {
        await adapter.clearCaches(ctx);
        if (ctx.daemon && adapter.hasDaemon) {
            await adapter.stopDaemon(ctx);
        }
    };

    // Emits the per-run scenario event plus a failure event on non-zero exits.
    const sampleEmitter =
        (scenario: "cold" | "warm", total: number) =>
        (run: number, sample: RunSample) => {
            emit({
                kind: "scenario",
                tool: adapter.tool,
                scenario,
                run,
                total,
                sample,
            });
            if (sample.exitCode !== 0) {
                emit({ kind: "tool-unsuccessful", tool: adapter.tool, sample });
            }
        };

    try {
        // In no-daemon mode make sure no stale daemon lingers first.
        if (!ctx.daemon && adapter.hasDaemon) {
            await adapter.stopDaemon(ctx);
        }

        // Cold: a fresh start each run — caches + outputs wiped, and (in daemon
        // mode) the daemon torn down so cold includes its startup cost.
        const coldSamples = await runScenario(
            runOnce,
            coldRuns,
            sampleEmitter("cold", coldRuns),
            coldBefore,
        );

        // Warm: prime once (unmeasured, also warms the daemon), then measure
        // while caches + daemon stay hot.
        await runOnce();
        const warmSamples = await runScenario(
            runOnce,
            warmRuns,
            sampleEmitter("warm", warmRuns),
        );

        // Resource usage is collected in dedicated passes (never during timed
        // runs) so the resource probe can't perturb the timing numbers. Warm
        // passes run first while caches + daemon are still hot; cold passes
        // reuse the cold reset hook. One warmed sampler is reused across all
        // passes so its startup cost is paid once.
        let coldResources: ResourceStats | undefined;
        let warmResources: ResourceStats | undefined;
        if (resourceRuns > 0) {
            const probe = createProcessProbe();
            try {
                await probe.warmup();
                warmResources = resourceStatsFrom(
                    await runResource(probe, resourceRuns),
                );
                coldResources = resourceStatsFrom(
                    await runResource(probe, resourceRuns, coldBefore),
                );
            } finally {
                await probe.dispose();
            }
        }

        const taskGraphSize = Math.max(
            0,
            ...coldSamples.map((s) => s.executed),
            ...warmSamples.map((s) => s.executed),
        );

        return {
            tool: adapter.tool,
            task,
            taskGraphSize,
            cold: scenarioFromSamples(coldSamples, coldResources),
            warm: scenarioFromSamples(warmSamples, warmResources),
        };
    } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        emit({ kind: "tool-error", tool: adapter.tool, error: message });
        return {
            tool: adapter.tool,
            task,
            taskGraphSize: 0,
            cold: scenarioFromSamples([]),
            warm: scenarioFromSamples([]),
            error: message,
        };
    } finally {
        // Always clean up so nothing leaks between tools or outlives the run.
        await adapter.stopDaemon(ctx);
        await adapter.clearCaches(ctx);
    }
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
    const resourceRuns = options.resourceRuns ?? 3;
    const platform = getPlatformInfo();
    const concurrency =
        options.concurrency ?? Math.max(1, platform.cpus.length);
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
    const versions: Record<string, string | null> =
        Object.fromEntries(versionMap);

    const results: ToolResult[] = [];
    for (const tool of tools) {
        emit({ kind: "tool-start", tool });
        results.push(
            await benchmarkTool(
                getAdapter(tool),
                ctx,
                task,
                coldRuns,
                warmRuns,
                resourceRuns,
                execLog,
                emit,
            ),
        );
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
        platform,
    };
}
