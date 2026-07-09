import type { ToolInfo } from "../tools";
import { renderTable, renderVersionList } from "./format";
import type { BenchmarkResult, ScenarioResult, ToolResult } from "./index";
import { CACHE_HIT_THRESHOLD, cacheHitRatio, isFullyCached } from "./metrics";
import type { PlatformInfo } from "./platform-info";
import { formatBytes, formatMs, type Stats } from "./stats";

/**
 * Render a per-tool attribute list (daemon, provisioning, supported versions,
 * plus a short description) so readers can interpret the numbers in context.
 * Returns just the bullet lines; callers supply their own label/heading.
 */
export function renderToolInfo(infos: ToolInfo[]): string[] {
    const lines: string[] = [];
    for (const info of infos) {
        const attrs = [
            `daemon: ${info.daemon ? "yes" : "no"}`,
            `provisioning: ${info.provisioning}`,
            `supported: ${info.supportedVersions.join(" || ") || "?"}`,
        ].join(" \u00b7 ");
        lines.push(`* **${info.tool}** ${info.version ?? "?"} — ${attrs}`);
        lines.push(`  ${info.description}`);
    }
    return lines;
}

function statCell(stats: Stats, failures: number): string {
    if (stats.samples.length === 0) return "—";
    const base = `${formatMs(stats.median)} ±${formatMs(stats.stddev)}`;
    return failures > 0 ? `${base} ⚠${failures}` : base;
}

/** Median peak RSS for a scenario (or "—" when not measured). */
function memCell(scenario: ScenarioResult): string {
    const r = scenario.resources;
    return r ? formatBytes(r.peakRssBytes.median) : "—";
}

/** Median CPU time + average parallelism for a scenario (or "—"). */
function cpuCell(scenario: ScenarioResult): string {
    const r = scenario.resources;
    if (!r) return "—";
    return `${formatMs(r.cpuTimeMs.median)} (${r.parallelism.median.toFixed(1)}×)`;
}

function cacheCell(tool: ToolResult): string {
    const ratio = cacheHitRatio(tool);
    return ratio === null ? "—" : `${(ratio * 100).toFixed(0)}%`;
}

/** "<label>: X is fastest (…), N× faster than Y (…)." or null if < 2 tools. */
function overheadLine(
    label: string,
    tools: ToolResult[],
    medianOf: (tool: ToolResult) => number,
): string | null {
    const ranked = [...tools].sort((a, b) => medianOf(a) - medianOf(b));
    const fastest = ranked[0];
    const slowest = ranked[ranked.length - 1];
    if (ranked.length < 2 || !fastest || !slowest) return null;
    const factor = medianOf(slowest) / medianOf(fastest);
    return (
        `${label}: ${fastest.tool} is fastest (${formatMs(medianOf(fastest))}), ` +
        `${factor.toFixed(2)}× faster than ${slowest.tool} ` +
        `(${formatMs(medianOf(slowest))}).`
    );
}

export function renderPlatformInfo(platform: PlatformInfo): string[] {
    const lines: string[] = [];

    lines.push("Platform: ");

    lines.push(
        `* OS: ${platform.os.platform} ${platform.os.release} (${platform.os.arch})`,
    );

    const cpuModelSet = new Set(platform.cpus.map((cpu) => cpu.model));
    const cpuSpeedSet = new Set(platform.cpus.map((cpu) => cpu.speedMHz));

    if (cpuModelSet.size === 1 && cpuSpeedSet.size === 1) {
        lines.push(
            `* CPU: ${platform.cpus.length} × ${platform.cpus[0]?.model ?? "unknown"} @ ${platform.cpus[0]?.speedMHz ?? "unknown"} MHz`,
        );
    } else {
        lines.push(
            `* CPU: ${platform.cpus.length} × [${[...cpuModelSet].join(", ")}] @ [${[...cpuSpeedSet].join(", ")} MHz]`,
        );
    }

    lines.push(
        `* Memory: ${Math.round(platform.memory.totalBytes / (1024 * 1024))} MB total, ${Math.round(platform.memory.freeBytes / (1024 * 1024))} MB free`,
    );

    return lines;
}

/**
 * Render a human-readable comparison table plus short takeaways. The warm
 * column is the key metric: with all tasks cached it approximates each tool's
 * discovery + cache-restore overhead. The warm-cache-hit column verifies that
 * assumption held (should be 100%). The resource columns (mem/cpu) are only
 * included when at least one tool reported resource usage.
 */
export function renderReport(
    result: BenchmarkResult,
    options: { includeToolInfo?: boolean } = {},
): string[] {
    const includeToolInfo = options.includeToolInfo ?? true;
    const lines: string[] = [];
    const graphSize = Math.max(0, ...result.tools.map((t) => t.taskGraphSize));
    lines.push("");
    lines.push(
        `* task-bench: ${result.projects} projects × ${result.tasksPerProject} tasks ` +
            `(${graphSize} task-graph nodes), running "${result.task}" ` +
            `at concurrency ${result.concurrency} ` +
            `(daemons ${result.daemon ? "on" : "off"})`,
    );
    lines.push(
        ...renderVersionList(
            result.tools.map((t) => [t.tool, result.versions[t.tool]] as const),
            "* versions",
        ),
    );
    lines.push("");
    lines.push(...renderPlatformInfo(result.platform));
    lines.push("");

    const toolInfo = result.toolInfo ?? [];
    if (includeToolInfo && toolInfo.length > 0) {
        lines.push("Tool info:");
        lines.push(...renderToolInfo(toolInfo));
        lines.push("");
    }

    // Only surface the resource columns when at least one tool reported
    // resource usage. If every entry lacks RSS/CPU data (e.g. resourceRuns was
    // 0), those columns would be all "—", so we drop them entirely. When any
    // tool has data we keep the columns and fill gaps with "—".
    const hasResourceInfo = result.tools.some(
        (t) => t.cold.resources || t.warm.resources,
    );

    const headers = [
        "tool",
        "cold (median)",
        "warm (median)",
        ...(hasResourceInfo
            ? ["cold mem", "warm mem", "cold cpu", "warm cpu"]
            : []),
        "warm cache-hit",
        "notes",
    ];
    const rows = result.tools.map((t) => [
        t.tool,
        statCell(t.cold.stats, t.cold.failures),
        statCell(t.warm.stats, t.warm.failures),
        ...(hasResourceInfo
            ? [
                  memCell(t.cold),
                  memCell(t.warm),
                  cpuCell(t.cold),
                  cpuCell(t.warm),
              ]
            : []),
        cacheCell(t),
        t.error ? `error: ${(t.error.split("\n")[0] ?? "").slice(0, 40)}` : "",
    ]);
    lines.push(...renderTable(headers, rows));
    lines.push("");

    // Warn if any warm scenario was not a full cache hit — the warm numbers
    // are only meaningful as "overhead" when everything is cached.
    const impure = result.tools.filter((t) => {
        const ratio = cacheHitRatio(t);
        return ratio !== null && ratio < CACHE_HIT_THRESHOLD;
    });
    for (const t of impure) {
        lines.push(
            `⚠ ${t.tool}: warm runs were not fully cached ` +
                `(${cacheCell(t)} hit, ${t.warm.executedMedian}/${t.taskGraphSize} tasks re-ran) — ` +
                `treat its warm number with caution.`,
        );
    }
    if (impure.length) lines.push("");

    // Warm takeaway is restricted to fully-cached, error-free tools; cold uses
    // every error-free tool (cold always executes, regardless of caching).
    const warmLine = overheadLine(
        "Warm-cache overhead",
        result.tools.filter((t) => !t.error && isFullyCached(t)),
        (t) => t.warm.stats.median,
    );
    const coldLine = overheadLine(
        "Cold-run overhead",
        result.tools.filter((t) => !t.error && t.cold.stats.samples.length > 0),
        (t) => t.cold.stats.median,
    );
    if (warmLine) lines.push(warmLine, "");
    if (coldLine) lines.push(coldLine, "");

    return lines;
}
