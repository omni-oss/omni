import type { ChartSpec, Report, SeriesSpec } from "../chart/ir";
import { formatValue } from "../format";

/**
 * Generic, data-derived analysis: short Markdown sentences describing each
 * chart and the report overall. Works purely off the Chart IR (series/points/
 * units/kind/emphasis), so it stays decoupled from the benchmark domain.
 *
 * `annotateReport` fills `chart.analysis` and `report.analysis` in place.
 * See DESIGN.md §6.
 */

/** Fill analysis fields on every chart and the report (mutates in place). */
export function annotateReport(report: Report): void {
    for (const view of report.views) {
        for (const chart of view.charts) {
            const a = analyzeChart(chart);
            if (a) chart.analysis = a;
        }
    }
    const overall = analyzeReport(report);
    if (overall) report.analysis = overall;
}

/** One short Markdown sentence describing a single chart. */
export function analyzeChart(chart: ChartSpec): string | undefined {
    const withData = chart.series.filter((s) =>
        s.points.some((p) => p.y !== null),
    );
    if (withData.length === 0) return undefined;
    return chart.kind === "line"
        ? trendAnalysis(chart, withData)
        : rankingAnalysis(chart, withData);
}

/** Trend over an ordered x-axis (e.g. versions): average % change first→last. */
function trendAnalysis(
    chart: ChartSpec,
    series: SeriesSpec[],
): string | undefined {
    const changes: number[] = [];
    let firstX: string | number | undefined;
    let lastX: string | number | undefined;
    for (const s of series) {
        const pts = s.points.filter((p) => p.y !== null);
        const a = pts[0];
        const b = pts[pts.length - 1];
        if (!a || !b || a === b || a.y === 0 || a.y === null || b.y === null) {
            continue;
        }
        changes.push(((b.y - a.y) / a.y) * 100);
        firstX = a.x;
        lastX = b.x;
    }
    if (changes.length === 0 || firstX === undefined || lastX === undefined) {
        return undefined;
    }
    const avg = changes.reduce((sum, n) => sum + n, 0) / changes.length;
    const abs = Math.abs(avg).toFixed(0);
    if (avg <= -1) {
        return `${chart.y.label} **improved ~${abs}%** from \`${firstX}\` to \`${lastX}\`.`;
    }
    if (avg >= 1) {
        return `${chart.y.label} **regressed ~${abs}%** from \`${firstX}\` to \`${lastX}\`.`;
    }
    return `${chart.y.label} held roughly flat from \`${firstX}\` to \`${lastX}\`.`;
}

/** Ranking across series (e.g. tools): lowest wins; highlight the emphasized one. */
function rankingAnalysis(
    chart: ChartSpec,
    series: SeriesSpec[],
): string | undefined {
    const reps = series
        .map((s) => ({ s, r: representative(s) }))
        .filter((x): x is { s: SeriesSpec; r: number } => x.r !== null)
        .sort((a, b) => a.r - b.r);
    const best = reps[0];
    if (!best) return undefined;

    const label = chart.y.label;
    const emphasized = reps.find((x) => x.s.emphasis);

    if (!emphasized) {
        return `**${best.s.label}** had the lowest ${label} (${formatValue(best.r, chart.y.unit)}).`;
    }
    if (emphasized === best) {
        const second = reps[1];
        if (!second) {
            return `**${emphasized.s.label}** had the lowest ${label} (${formatValue(best.r, chart.y.unit)}).`;
        }
        const ahead = pct(second.r, best.r);
        return `**${emphasized.s.label}** had the lowest ${label} — ~${ahead}% below **${second.s.label}**.`;
    }
    const behind = pct(best.r, emphasized.r);
    return `**${best.s.label}** led ${label}; **${emphasized.s.label}** was ~${behind}% higher.`;
}

/** Overall summary: a data-derived finding across the whole report. */
function analyzeReport(report: Report): string | undefined {
    const parts: string[] = [];
    const spotlight =
        typeof report.meta.spotlight === "string"
            ? report.meta.spotlight
            : null;
    if (spotlight) parts.push(`Spotlighting **omni ${spotlight}**.`);

    let omniBest = 0;
    let total = 0;
    for (const view of report.views) {
        for (const chart of view.charts) {
            if (chart.kind === "line") continue;
            const reps = chart.series
                .map((s) => ({ s, r: representative(s) }))
                .filter((x): x is { s: SeriesSpec; r: number } => x.r !== null);
            const emph = reps.find((x) => x.s.emphasis);
            if (!emph || reps.length < 2) continue;
            total++;
            const min = Math.min(...reps.map((x) => x.r));
            if (emph.r === min) omniBest++;
        }
    }
    if (total > 0) {
        parts.push(
            `omni had the lowest median in **${omniBest}/${total}** tool comparison(s).`,
        );
    }
    return parts.length > 0 ? parts.join(" ") : undefined;
}

/** Representative value for a series: median of its non-null points. */
function representative(series: SeriesSpec): number | null {
    const ys = series.points
        .map((p) => p.y)
        .filter((y): y is number => y !== null);
    if (ys.length === 0) return null;
    const sorted = [...ys].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2
        : (sorted[mid] ?? 0);
}

/** Absolute % difference of `to` relative to `from`, rounded. */
function pct(from: number, to: number): string {
    if (from === 0) return "0";
    return Math.abs(((to - from) / from) * 100).toFixed(0);
}
