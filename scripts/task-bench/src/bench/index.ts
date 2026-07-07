import { writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { execa } from "execa";
import { HarnessConfigSchema, type Tool } from "../config";
import { buildModel, expectedColdExecuted, taskNames } from "../model";
import {
    describeTool,
    getAdapter,
    resolveToolVersions,
    type ToolAdapter,
    type ToolContext,
    type ToolInfo,
} from "../tools";
import { BASE_ENV } from "./env";
import { getPlatformInfo, type PlatformInfo } from "./platform-info";
import { createProcessProbe, type ProcessProbe } from "./process-probe";
import {
    countExecLogLines,
    measureRun,
    type ResourceSample,
} from "./resource-usage";
import { computeStats, median, type Stats } from "./stats";
import { unrecoverableExitReason } from "./unrecoverable";

/**
 * Success + verification signals shared by timed ({@link RunSample}) and
 * resource ({@link ResourceSample}) passes, so both can flow through the same
 * retry machinery.
 */
export interface RunOutcome {
    /** Whether the invocation exited cleanly. */
    ok: boolean;
    exitCode: number;
    /** Number of tasks that actually executed (0 == a full cache hit). */
    executed: number;
}

export interface RunSample extends RunOutcome {
    durationMs: number;
    stdout: string;
    stderr: string;
}

/** Default number of extra attempts for a run that fails or fails verification. */
export const DEFAULT_MAX_RETRIES = 2;

/** Why a measured run was rejected and (possibly) retried. */
export type RetryReason = "exit" | "verification";

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
    /** Noteworthy attributes of each benchmarked tool (daemon, provisioning, …). */
    toolInfo: ToolInfo[];
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
    | { kind: "tool-unsuccessful"; tool: Tool; sample: RunSample }
    | {
          kind: "run-retry";
          tool: Tool;
          scenario: "cold" | "warm";
          /** Which phase the retried invocation belongs to. */
          phase: "timed" | "resource";
          run: number;
          /** Which attempt just failed (1 == the first, pre-retry attempt). */
          attempt: number;
          maxRetries: number;
          reason: RetryReason;
          sample: RunOutcome;
      }
    | {
          kind: "run-aborted";
          tool: Tool;
          scenario: "cold" | "warm";
          phase: "timed" | "resource";
          run: number;
          /** Which attempt hit the unrecoverable exit code. */
          attempt: number;
          exitCode: number;
          /** Why the code is treated as unrecoverable (from the code map). */
          reason: string;
          sample: RunOutcome;
      };

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
    /**
     * Extra attempts for a measured run that exits non-zero or fails its
     * scenario's verification (cold must execute the expected number of tasks,
     * warm must be fully cached). Defaults to {@link DEFAULT_MAX_RETRIES}. 0
     * disables retries.
     */
    maxRetries?: number | undefined;
    onEvent?: ((event: BenchEvent) => void) | undefined;
}

/** A zero-argument runner that executes one measured invocation. */
type RunOnce = () => Promise<RunSample>;

/**
 * Per-scenario retry policy for timed runs. A run is accepted when it exits
 * cleanly *and* satisfies `verify`; otherwise it is retried (re-running
 * `before` first, so a cold reset is reapplied) until it passes or the
 * attempts are exhausted. A run that fails with an unrecoverable exit code
 * (see `unrecoverableExitReason`) is never retried — `onAbort` fires instead.
 */
interface RetryPolicy {
    maxRetries: number;
    /** Scenario-specific check (e.g. warm runs must be fully cached). */
    verify: (sample: RunOutcome) => boolean;
    onRetry: (
        run: number,
        attempt: number,
        sample: RunSample,
        reason: RetryReason,
    ) => void;
    onAbort: (
        run: number,
        attempt: number,
        sample: RunSample,
        reason: string,
    ) => void;
}

/** Reason a sample is unacceptable, or null when it passed all checks. */
function rejectionReason<T extends RunOutcome>(
    sample: T,
    verify: (sample: T) => boolean,
): RetryReason | null {
    if (!sample.ok) return "exit";
    if (!verify(sample)) return "verification";
    return null;
}

/**
 * Verification requirement for a scenario. A warm pass must be a full cache hit
 * (nothing re-ran). A cold pass must execute exactly `expectedCold` tasks (the
 * whole graph re-ran); when that count is unknown (`null`) it falls back to
 * "at least one task ran", which still catches a failed cache wipe. Works for
 * both timed and resource samples via {@link RunOutcome}.
 */
function verifyScenario(
    scenario: "cold" | "warm",
    expectedCold: number | null,
) {
    return (sample: RunOutcome): boolean => {
        if (scenario === "warm") return sample.executed === 0;
        return expectedCold === null
            ? sample.executed > 0
            : sample.executed === expectedCold;
    };
}

/**
 * Run a single measured invocation, retrying while it exits non-zero or fails
 * `verify`. `before` (if any) runs before every attempt so a cold reset is
 * reapplied on each retry. Retrying stops early (and `onAbort` fires) when the
 * failure is an unrecoverable host error, since retrying would be futile or
 * harmful. The final attempt's sample is returned even if it never passed, so
 * an exhausted budget (or an abort) still surfaces downstream.
 */
async function runWithRetry<T extends RunOutcome>(
    once: () => Promise<T>,
    verify: (sample: T) => boolean,
    maxRetries: number,
    onRetry: (attempt: number, sample: T, reason: RetryReason) => void,
    onAbort: (attempt: number, sample: T, reason: string) => void,
    before?: () => Promise<void>,
): Promise<T> {
    let attempt = 0;
    for (;;) {
        if (before) await before();
        const sample = await once();
        attempt++;
        const reason = rejectionReason(sample, verify);
        if (reason === null) return sample;
        // Host-level failures won't recover by retrying — and on Windows a
        // retry deepens the process-exhaustion that caused it — so bail out.
        const unrecoverable = unrecoverableExitReason(sample.exitCode);
        if (unrecoverable !== null) {
            onAbort(attempt, sample, unrecoverable);
            return sample;
        }
        if (attempt > maxRetries) return sample;
        onRetry(attempt, sample, reason);
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
        executed: countExecLogLines(execLog),
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

/**
 * Run one scenario `runs` times, calling `before` (if any) before each attempt.
 * Each run is retried per `retry` while it exits non-zero or fails verification;
 * the final attempt's sample is kept (and still counted) even if it never
 * passed, so an exhausted retry budget surfaces in the reported failures.
 */
async function runScenario(
    runOnce: RunOnce,
    runs: number,
    onSample: (run: number, sample: RunSample) => void,
    retry: RetryPolicy,
    before?: () => Promise<void>,
): Promise<RunSample[]> {
    const samples: RunSample[] = [];
    for (let run = 1; run <= runs; run++) {
        const sample = await runWithRetry(
            runOnce,
            retry.verify,
            retry.maxRetries,
            (attempt, s, reason) => retry.onRetry(run, attempt, s, reason),
            (attempt, s, reason) => retry.onAbort(run, attempt, s, reason),
            before,
        );
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
    maxRetries: number,
    expectedCold: number | null,
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
        scenario: "cold" | "warm",
        before?: () => Promise<void>,
    ): Promise<ResourceSample[]> => {
        const samples: ResourceSample[] = [];
        for (let i = 0; i < runs; i++) {
            const run = i + 1;
            const sample = await runWithRetry(
                () => measureOnce(probe),
                verifyScenario(scenario, expectedCold),
                maxRetries,
                (attempt, s, reason) =>
                    emit({
                        kind: "run-retry",
                        tool: adapter.tool,
                        scenario,
                        phase: "resource",
                        run,
                        attempt,
                        maxRetries,
                        reason,
                        sample: s,
                    }),
                (attempt, s, reason) =>
                    emit({
                        kind: "run-aborted",
                        tool: adapter.tool,
                        scenario,
                        phase: "resource",
                        run,
                        attempt,
                        exitCode: s.exitCode,
                        reason,
                        sample: s,
                    }),
                before,
            );
            samples.push(sample);
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

    // Verification requirements per scenario: a cold run must execute the whole
    // task graph (the expected count), a warm run must be a full cache hit
    // (nothing re-ran). Failing either — or a non-zero exit — triggers a retry.
    const retryPolicy = (scenario: "cold" | "warm"): RetryPolicy => ({
        maxRetries,
        verify: verifyScenario(scenario, expectedCold),
        onRetry: (run, attempt, sample, reason) =>
            emit({
                kind: "run-retry",
                tool: adapter.tool,
                scenario,
                phase: "timed",
                run,
                attempt,
                maxRetries,
                reason,
                sample,
            }),
        onAbort: (run, attempt, sample, reason) =>
            emit({
                kind: "run-aborted",
                tool: adapter.tool,
                scenario,
                phase: "timed",
                run,
                attempt,
                exitCode: sample.exitCode,
                reason,
                sample,
            }),
    });

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
            retryPolicy("cold"),
            coldBefore,
        );

        // Warm: prime once (unmeasured, also warms the daemon), then measure
        // while caches + daemon stay hot.
        await runOnce();
        const warmSamples = await runScenario(
            runOnce,
            warmRuns,
            sampleEmitter("warm", warmRuns),
            retryPolicy("warm"),
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
                    await runResource(probe, resourceRuns, "warm"),
                );
                coldResources = resourceStatsFrom(
                    await runResource(probe, resourceRuns, "cold", coldBefore),
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
    const model = buildModel(config);
    const projects = model.projects;
    const tasks = taskNames(config);

    const tools = options.tools ?? config.tools;
    const task = options.task ?? tasks[tasks.length - 1] ?? "t0";
    const coldRuns = options.coldRuns ?? 3;
    const warmRuns = options.warmRuns ?? 5;
    const resourceRuns = options.resourceRuns ?? 3;
    const maxRetries = Math.max(0, options.maxRetries ?? DEFAULT_MAX_RETRIES);
    // Expected task count for a correct cold run of `task`, used to verify that
    // cold runs re-ran the whole graph (and to retry them when they didn't).
    const expectedCold = expectedColdExecuted(model, task);
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
    const toolInfo: ToolInfo[] = tools.map((tool) =>
        describeTool(tool, config, versionMap.get(tool) ?? null),
    );

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
                maxRetries,
                expectedCold,
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
        toolInfo,
        generatedAt: new Date().toISOString(),
        tools: results,
        platform,
    };
}
