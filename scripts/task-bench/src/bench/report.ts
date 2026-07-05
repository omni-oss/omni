import type { BenchmarkResult, ToolResult } from "./index";
import { formatMs, type Stats } from "./stats";

function pad(value: string, width: number): string {
    return value.length >= width
        ? value
        : value + " ".repeat(width - value.length);
}

function statCell(stats: Stats, failures: number): string {
    if (stats.samples.length === 0) return "—";
    const base = `${formatMs(stats.median)} ±${formatMs(stats.stddev)}`;
    return failures > 0 ? `${base} ⚠${failures}` : base;
}

function cacheHitPct(tool: ToolResult): number | null {
    if (tool.taskGraphSize <= 0 || tool.warm.stats.samples.length === 0) {
        return null;
    }
    const hits = tool.taskGraphSize - tool.warm.executedMedian;
    return (hits / tool.taskGraphSize) * 100;
}

function cacheCell(tool: ToolResult): string {
    const pct = cacheHitPct(tool);
    if (pct === null) return "—";
    return `${pct.toFixed(0)}%`;
}

/**
 * Render a human-readable comparison table plus a short takeaway. The warm
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
    lines.push(formatVersions(result));
    lines.push("");

    const headers = [
        "tool",
        "cold (median)",
        "warm (median)",
        "warm cache-hit",
        "notes",
    ];
    const rows = result.tools.map((t) => {
        const notes = t.error
            ? `error: ${(t.error.split("\n")[0] ?? "").slice(0, 40)}`
            : "";
        return [
            t.tool,
            statCell(t.cold.stats, t.cold.failures),
            statCell(t.warm.stats, t.warm.failures),
            cacheCell(t),
            notes,
        ];
    });

    const widths = headers.map((h, i) =>
        Math.max(h.length, ...rows.map((r) => (r[i] ?? "").length)),
    );

    const renderRow = (cells: string[]) =>
        `| ${cells.map((c, i) => pad(c, widths[i] ?? 0)).join(" | ")} |`;

    lines.push(renderRow(headers));
    lines.push(`| ${widths.map((w) => "-".repeat(w)).join(" | ")} |`);
    for (const row of rows) lines.push(renderRow(row));
    lines.push("");

    // Warn if any warm scenario was not a full cache hit — the warm numbers
    // are only meaningful as "overhead" when everything is cached.
    const impure = result.tools.filter((t) => {
        const pct = cacheHitPct(t);
        return pct !== null && pct < 99.5;
    });
    for (const t of impure) {
        lines.push(
            `⚠ ${t.tool}: warm runs were not fully cached ` +
                `(${cacheCell(t)} hit, ${t.warm.executedMedian}/${t.taskGraphSize} tasks re-ran) — ` +
                `treat its warm number with caution.`,
        );
    }
    if (impure.length) lines.push("");

    // Fastest-warm takeaway (only among fully-cached, error-free tools).
    const ranked = result.tools
        .filter((t) => {
            const pct = cacheHitPct(t);
            return !t.error && pct !== null && pct >= 99.5;
        })
        .sort((a, b) => a.warm.stats.median - b.warm.stats.median);
    const fastest = ranked[0];
    const slowest = ranked[ranked.length - 1];
    if (ranked.length > 1 && fastest && slowest) {
        const factor = slowest.warm.stats.median / fastest.warm.stats.median;
        lines.push(
            `Warm-cache overhead: ${fastest.tool} is fastest ` +
                `(${formatMs(fastest.warm.stats.median)}), ` +
                `${factor.toFixed(2)}× faster than ${slowest.tool} ` +
                `(${formatMs(slowest.warm.stats.median)}).\n`,
        );
    }

    // Fastest-cold takeaway (discovery + full execution + cache write).
    const coldRanked = result.tools
        .filter((t) => !t.error && t.cold.stats.samples.length > 0)
        .sort((a, b) => a.cold.stats.median - b.cold.stats.median);
    const coldFastest = coldRanked[0];
    const coldSlowest = coldRanked[coldRanked.length - 1];
    if (coldRanked.length > 1 && coldFastest && coldSlowest) {
        const factor =
            coldSlowest.cold.stats.median / coldFastest.cold.stats.median;
        lines.push(
            `Cold-run overhead: ${coldFastest.tool} is fastest ` +
                `(${formatMs(coldFastest.cold.stats.median)}), ` +
                `${factor.toFixed(2)}× faster than ${coldSlowest.tool} ` +
                `(${formatMs(coldSlowest.cold.stats.median)}).`,
        );
    }
    if (ranked.length > 1 || coldRanked.length > 1) lines.push("");

    return lines.join("\n");
}

/** A one-line summary of the resolved tool versions used. */
function formatVersions(result: BenchmarkResult): string {
    const parts = result.tools.map(
        (t) => `${t.tool} ${result.versions[t.tool] ?? "?"}`,
    );
    return `versions: ${parts.join(", ")}`;
}
