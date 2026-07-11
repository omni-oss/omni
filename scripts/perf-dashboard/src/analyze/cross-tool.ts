import type { ChartSpec, Dashboard, InfoGroup, SeriesSpec } from "../chart/ir";
import type {
    Metric,
    NormalizedRun,
    SamplePoint,
    Tool,
    Warmth,
} from "../ingest/model";
import type { TargetId } from "../sources/types";
import { platformByTargetGroup, toolsInfoGroup } from "./info";
import { isUsable, metricAxis, metricLabel, unique } from "./select";

/**
 * Cross-tool comparison: one omni version vs. the other tools in the same runs.
 * Tolerant of ragged data — plots whatever tools/targets are present, omits
 * absent/errored tools, and emits resource charts only where resource data
 * exists for omni. See DESIGN.md §6.1.
 */

export interface CrossToolOptions {
    /** The omni version to spotlight. */
    version: string;
    /** Restrict to these targets; omit ⇒ all present for the version. */
    targets?: TargetId[];
}

const WARMTHS: Warmth[] = ["warm", "cold"];
const OMNI: Tool = "omni";

export function crossTool(
    runs: NormalizedRun[],
    opts: CrossToolOptions,
): Dashboard {
    const scoped = runs.filter(
        (r) =>
            r.version === opts.version &&
            (!opts.targets || opts.targets.includes(r.target)),
    );

    const targets = unique(scoped.map((r) => r.target)).sort((a, b) =>
        a.localeCompare(b),
    );

    const charts: ChartSpec[] = [];
    const toolsSeen = new Set<string>();

    for (const target of targets) {
        const points = scoped
            .filter((r) => r.target === target)
            .flatMap((r) => r.points);
        if (points.length === 0) continue;

        for (const p of points) toolsSeen.add(p.coord.tool);

        const scenarios = unique(points.map((p) => p.coord.scenario)).sort(
            (a, b) => a.localeCompare(b),
        );

        const metrics: Metric[] = ["durationMs"];
        const omniHasResources = points.some(
            (p) => p.coord.tool === OMNI && p.hasResources,
        );
        if (omniHasResources) metrics.push("peakRssBytes", "cpuTimeMs");

        for (const metric of metrics) {
            for (const warmth of WARMTHS) {
                const chart = buildChart(
                    target,
                    metric,
                    warmth,
                    points,
                    scenarios,
                );
                if (chart) charts.push(chart);
            }
        }
    }

    const notes = [
        `Spotlight: omni ${opts.version}.`,
        `Tools present: ${[...toolsSeen].sort().join(", ") || "none"}.`,
        `Targets present: ${targets.join(", ") || "none"}.`,
    ];

    const info = [
        toolsInfoGroup(scoped[0]?.toolInfo ?? []),
        platformByTargetGroup(scoped),
    ].filter((g): g is InfoGroup => g !== null);

    return {
        id: "cross-tool",
        kind: "cross-tool",
        title: `omni ${opts.version} vs. other tools`,
        description:
            "Task-execution overhead of omni against the other runners in the same runs.",
        generatedAt: new Date().toISOString(),
        meta: {
            version: opts.version,
            targets,
            tools: [...toolsSeen].sort(),
        },
        charts,
        notes,
        ...(info.length > 0 ? { info } : {}),
    };
}

function buildChart(
    target: TargetId,
    metric: Metric,
    warmth: Warmth,
    points: SamplePoint[],
    scenarios: string[],
): ChartSpec | null {
    const relevant = points.filter(
        (p) => p.metric === metric && p.coord.warmth === warmth,
    );

    // Tools with at least one usable point become series (omni always, if seen).
    const usableTools = new Set<string>(
        relevant.filter(isUsable).map((p) => p.coord.tool),
    );
    if (relevant.some((p) => p.coord.tool === OMNI)) usableTools.add(OMNI);

    const tools = [...usableTools].sort(toolOrder);
    if (tools.length === 0) return null;

    const series: SeriesSpec[] = tools.map((tool) => ({
        key: tool,
        label: tool,
        ...(tool === OMNI ? { emphasis: true } : {}),
        points: scenarios.map((scenario) => {
            const point = relevant.find(
                (p) => p.coord.tool === tool && p.coord.scenario === scenario,
            );
            const usable = point && isUsable(point);
            return {
                x: scenario,
                y: usable ? point.median : null,
                ...(usable && metric === "durationMs" && point.stddev > 0
                    ? { yError: point.stddev }
                    : {}),
            };
        }),
    }));

    // Skip a chart nobody has data for.
    if (!series.some((s) => s.points.some((pt) => pt.y !== null))) return null;

    return {
        id: `${target}--${metric}--${warmth}`,
        kind: "grouped-bar",
        title: `${capitalize(warmth)} ${metricLabel(metric)} — ${target}`,
        x: { label: "scenario" },
        y: metricAxis(metric),
        series,
        facets: [
            { dimension: "target", value: target },
            { dimension: "metric", value: metricLabel(metric) },
            { dimension: "warmth", value: warmth },
        ],
    };
}

/** omni first, then the rest alphabetically. */
function toolOrder(a: string, b: string): number {
    if (a === OMNI) return -1;
    if (b === OMNI) return 1;
    return a.localeCompare(b);
}

function capitalize(s: string): string {
    return s.charAt(0).toUpperCase() + s.slice(1);
}
