import { formatVersionList, renderTable } from "./format";
import type { BenchmarkResult, ToolResult } from "./index";
import { CACHE_HIT_THRESHOLD, cacheHitRatio, isFullyCached } from "./metrics";
import { formatMs, type Stats } from "./stats";

function statCell(stats: Stats, failures: number): string {
    if (stats.samples.length === 0) return "—";
    const base = `${formatMs(stats.median)} ±${formatMs(stats.stddev)}`;
    return failures > 0 ? `${base} ⚠${failures}` : base;
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

/**
 * Render a human-readable comparison table plus short takeaways. The warm
 * column is the key metric: with all tasks cached it approximates each tool's
 * discovery + cache-restore overhead. The warm-cache-hit column verifies that
 * assumption held (should be 100%).
 */
export function formatReport(result: BenchmarkResult): string {
    const lines: string[] = [];
    const graphSize = Math.max(0, ...result.tools.map((t) => t.taskGraphSize));
    lines.push("");
    lines.push(
        `task-bench: ${result.projects} projects × ${result.tasksPerProject} tasks ` +
            `(${graphSize} task-graph nodes), running "${result.task}" ` +
            `at concurrency ${result.concurrency} ` +
            `(daemons ${result.daemon ? "on" : "off"})`,
    );
    lines.push(
        formatVersionList(
            result.tools.map((t) => [t.tool, result.versions[t.tool]] as const),
            "versions",
        ),
    );
    lines.push("");

    const headers = [
        "tool",
        "cold (median)",
        "warm (median)",
        "warm cache-hit",
        "notes",
    ];
    const rows = result.tools.map((t) => [
        t.tool,
        statCell(t.cold.stats, t.cold.failures),
        statCell(t.warm.stats, t.warm.failures),
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

    return lines.join("\n");
}
