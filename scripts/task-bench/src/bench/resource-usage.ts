import { readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { execa } from "execa";
import { BASE_ENV } from "./env";
import { descendantPids, type ProcessProbe } from "./process-probe";

/** Resource usage attributed to a single measured (unmeasured-for-timing) run. */
export interface ResourceSample {
    /** Peak summed RSS (bytes) across the whole invocation process tree. */
    peakRssBytes: number;
    /** Total CPU time (user+sys, ms) attributed to this run. */
    cpuTimeMs: number;
    /** Wall-clock duration (ms) of the invocation (not a timing metric). */
    wallMs: number;
    /** Derived average cores used = cpuTimeMs / wallMs. */
    parallelism: number;
    /** Exit code of the invocation (-1 if unknown). */
    exitCode: number;
    /** Whether the invocation exited cleanly. */
    ok: boolean;
    /** Number of tasks that actually executed (0 == a full cache hit). */
    executed: number;
    /**
     * PIDs of daemon processes measured alongside the CLI for this run:
     *   - pre-existing daemons resolved before the run starts (warm runs),
     *   - daemons discovered shortly after launch (cold runs).
     * Empty when no daemon was found or this tool has no daemon.
     */
    daemonPids: number[];
}

/** Count newline-terminated lines in the execution-marker log (0 on any error). */
export function countExecLogLines(path: string): number {
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

/** How often (ms) to re-discover the process tree; see `measureRun`. */
const TREE_REFRESH_MS = 400;

/**
 * Run one invocation while sampling the resource usage of the *entire process
 * tree* it spawns — the CLI process, its children (task workers), and any
 * persistent daemon — not just the CLI itself. This is a dedicated pass, kept
 * separate from timed runs so the probe can never inflate the `durationMs`
 * numbers the benchmark reports.
 *
 * Sampling is split into two cadences because on Windows a full tree query
 * (`Get-CimInstance`) costs ~800ms — longer than a warm run:
 *   - every `intervalMs` we cheaply sample the RSS/CPU of the PIDs we already
 *     track (roots + known descendants). Warm runs are 100% cache hits, so they
 *     spawn no task workers and the roots alone are the whole tree.
 *   - on a coarse `TREE_REFRESH_MS` cadence, off the critical path, we refresh
 *     the descendant set so cold runs pick up their (longer-lived) task workers.
 *
 * Two kinds of "daemon" PIDs are handled differently:
 *   - `daemonPids` resolved *before* the run are persistent across runs, so only
 *     their `ctime` delta (end − start) is attributed to this run.
 *   - a daemon the invocation starts itself (e.g. cold turbo/nx) is discovered
 *     via `resolveDaemonPids` shortly after launch and counted in full.
 *
 * CPU is each process's cumulative CPU-time counter, summed over the tree; RSS
 * is a sampled maximum of the summed footprint. Both are lower bounds: a process
 * that spawns and exits entirely between samples is missed.
 *
 * The `probe` is supplied (and disposed) by the caller so a single warmed-up
 * sampler is reused across a tool's passes — its startup (~1.5s on Windows) is
 * paid once rather than per run.
 */
export async function measureRun(
    probe: ProcessProbe,
    file: string,
    args: string[],
    rootDir: string,
    execLog: string,
    extraEnv: Record<string, string>,
    daemonPids: number[],
    resolveDaemonPids?: () => Promise<number[]>,
    intervalMs = 25,
): Promise<ResourceSample> {
    writeFileSync(execLog, "");
    // Ensure the sampler's backing helper is up *before* we launch, so the first
    // sample of a short run lands while it is still alive.
    await probe.warmup();
    // Persistent daemons known up-front only contribute their delta this run.
    const persistentDaemons = new Set(daemonPids);
    const daemonBaseline = new Map<number, number>();
    // Roots of the process tree (CLI + daemons); descendants grow from these.
    const roots = new Set(daemonPids);
    const tracked = new Set(daemonPids);
    // All non-CLI daemon PIDs resolved for this run (persistent + discovered).
    const daemonRoots = new Set<number>(daemonPids);
    const latestCtime = new Map<number, number>();
    let peakRssBytes = 0;

    const start = performance.now();
    const subprocess = execa(file, args, {
        cwd: rootDir,
        reject: false,
        env: { ...BASE_ENV, TASK_BENCH_EXEC_LOG: execLog, ...extraEnv },
    });
    if (subprocess.pid) {
        roots.add(subprocess.pid);
        tracked.add(subprocess.pid);
    }

    const sampleFast = async (): Promise<void> => {
        const stats = await probe.sample([...tracked]);
        let rss = 0;
        for (const [pid, s] of stats) {
            rss += s.rssBytes;
            latestCtime.set(pid, s.cpuMs);
            if (persistentDaemons.has(pid) && !daemonBaseline.has(pid)) {
                daemonBaseline.set(pid, s.cpuMs);
            }
        }
        if (rss > peakRssBytes) peakRssBytes = rss;
    };

    // Re-discover descendants of the roots and fold them into `tracked`. Run in
    // the background so its cost (slow on Windows) never stalls fast sampling.
    let treeBusy = false;
    const refreshTree = async (): Promise<void> => {
        if (treeBusy) return;
        treeBusy = true;
        try {
            const parents = await probe.parents();
            if (parents.size === 0) return;
            for (const pid of descendantPids(parents, roots)) tracked.add(pid);
        } catch {
            // best-effort
        } finally {
            treeBusy = false;
        }
    };

    // Immediate root sampling (fixes the "0" warm case), then kick off tree
    // discovery in the background.
    await sampleFast();
    void refreshTree();
    // If no persistent daemon was known up-front (cold run just tore it down),
    // try to discover a daemon this invocation starts for itself. It is counted
    // in full (not baselined) because it is fresh for this run.
    if (resolveDaemonPids && persistentDaemons.size === 0) {
        const discovered = await resolveDaemonPids().catch(() => []);
        for (const pid of discovered) {
            roots.add(pid);
            tracked.add(pid);
            daemonRoots.add(pid);
        }
    }

    let done = false;
    let lastTree = performance.now();
    const loop = (async () => {
        while (!done) {
            await delay(intervalMs);
            if (done) break;
            if (performance.now() - lastTree > TREE_REFRESH_MS) {
                lastTree = performance.now();
                void refreshTree();
            }
            await sampleFast();
        }
    })();

    const result = await subprocess;
    done = true;
    await loop;
    // Final reading: the CLI is gone (its last live sample stands), but any
    // daemon is still alive, so this captures its end-of-run CPU/RSS.
    await sampleFast();

    const wallMs = performance.now() - start;
    let cpuTimeMs = 0;
    for (const [pid, ctime] of latestCtime) {
        cpuTimeMs += persistentDaemons.has(pid)
            ? Math.max(0, ctime - (daemonBaseline.get(pid) ?? ctime))
            : ctime;
    }

    const exitCode = result.exitCode ?? -1;
    return {
        peakRssBytes,
        cpuTimeMs,
        wallMs,
        parallelism: wallMs > 0 ? cpuTimeMs / wallMs : 0,
        exitCode,
        ok: exitCode === 0,
        executed: countExecLogLines(execLog),
        daemonPids: [...daemonRoots],
    };
}
