import type {
    BuildProvenance,
    ChartSpec,
    Dashboard,
    ExcludedItem,
    SeriesSpec,
} from "../chart/ir";
import type { Metric, NormalizedRun, Warmth } from "../ingest/model";
import type { TargetId } from "../sources/types";
import { platformByVersionGroup } from "./info";
import { canonicalPreset, type PresetAliasMap } from "./preset-aliases";
import { canonicalScenario, type ScenarioAliasMap } from "./scenario-aliases";
import {
    compareVersions,
    groupBy,
    isUsable,
    metricAxis,
    metricLabel,
} from "./select";

/**
 * Version-to-version comparison: omni over its own release history, on a
 * normalized, apples-to-apples slice. Versions whose data doesn't meet the
 * minimum requirements are dropped and surfaced in a mandatory exclusion panel.
 * See DESIGN.md §6.2.
 */

export type MinDataCheck = "linux-present" | "full-preset" | "resource-runs";

export interface MinimumDataPolicy {
    os: string;
    target: TargetId;
    preset: string;
    requireResources: boolean;
}

export const DEFAULT_MIN_DATA: MinimumDataPolicy = {
    os: "linux",
    target: "x86_64-unknown-linux-gnu",
    preset: "full",
    requireResources: true,
};

export interface VersionVerdict {
    version: string;
    eligible: boolean;
    checks: Record<MinDataCheck, boolean>;
    reason?: string;
}

const OMNI = "omni";
const WARMTHS: Warmth[] = ["warm", "cold"];
const METRICS: Metric[] = ["durationMs", "peakRssBytes", "cpuTimeMs"];

export function versionHistory(
    runs: NormalizedRun[],
    policy: MinimumDataPolicy = DEFAULT_MIN_DATA,
    aliases: ScenarioAliasMap = {},
    presetAliases: PresetAliasMap = {},
): Dashboard {
    const byVersion = groupBy(runs, (r) => r.version);

    const verdicts: VersionVerdict[] = [];
    const kept = new Map<string, NormalizedRun>();

    for (const [version, vruns] of byVersion) {
        const baseline = vruns.find(
            (r) => r.os === policy.os || r.target === policy.target,
        );
        const checks: Record<MinDataCheck, boolean> = {
            "linux-present": baseline !== undefined,
            "full-preset":
                baseline !== undefined &&
                canonicalPreset(baseline.preset, presetAliases) ===
                    policy.preset,
            "resource-runs":
                baseline?.points.some(
                    (p) => p.coord.tool === OMNI && p.hasResources,
                ) ?? false,
        };
        const eligible =
            checks["linux-present"] &&
            checks["full-preset"] &&
            (!policy.requireResources || checks["resource-runs"]);

        const verdict: VersionVerdict = { version, eligible, checks };
        if (!eligible) verdict.reason = reasonFor(checks, policy);
        verdicts.push(verdict);

        if (eligible && baseline) kept.set(version, baseline);
    }

    const versions = [...kept.keys()].sort(compareVersions);

    // Canonical scenarios (omni, usable) present in every kept version.
    const commonScenarios = intersectScenarios(kept, versions, aliases);

    // version → canonical-scenario → warmth → metric → median (omni, usable).
    const lookup = buildLookup(kept, aliases);

    const charts: ChartSpec[] = [];
    for (const metric of METRICS) {
        for (const warmth of WARMTHS) {
            const chart = buildChart(
                metric,
                warmth,
                versions,
                commonScenarios,
                lookup,
            );
            if (chart) charts.push(chart);
        }
    }

    const excluded = verdicts.filter((v) => !v.eligible);
    const dashboard: Dashboard = {
        id: "version-history",
        kind: "version-history",
        title: "omni performance across versions",
        description:
            "Trends on a normalized slice (Linux · full preset · resource runs).",
        generatedAt: new Date().toISOString(),
        meta: {
            policy,
            keptVersions: versions,
            commonScenarios,
        },
        charts,
        provenance: versions.map((v) => provenanceFor(v, kept.get(v))),
    };

    if (excluded.length > 0) {
        dashboard.exclusionPanel = {
            title: "Excluded versions (insufficient data)",
            criteria: "Requires: Linux · full preset · resource runs",
            items: excluded.map(toExcludedItem),
        };
    }

    const platform = platformByVersionGroup(kept, versions);
    if (platform) dashboard.info = [platform];

    return dashboard;
}

function intersectScenarios(
    kept: Map<string, NormalizedRun>,
    versions: string[],
    aliases: ScenarioAliasMap,
): string[] {
    const perVersion: Set<string>[] = [];
    for (const version of versions) {
        const run = kept.get(version);
        if (!run) continue;
        perVersion.push(
            new Set(
                run.points
                    .filter((p) => p.coord.tool === OMNI && isUsable(p))
                    .map((p) => canonicalScenario(p.coord.scenario, aliases)),
            ),
        );
    }
    if (perVersion.length === 0) return [];

    let common: Set<string> = perVersion[0] ?? new Set<string>();
    for (const scenarios of perVersion.slice(1)) {
        common = new Set([...common].filter((s: string) => scenarios.has(s)));
    }
    return [...common].sort((a, b) => a.localeCompare(b));
}

type Lookup = Map<string, Map<string, number>>;

/** key: `${canonicalScenario}|${warmth}|${metric}` → median. */
function buildLookup(
    kept: Map<string, NormalizedRun>,
    aliases: ScenarioAliasMap,
): Lookup {
    const lookup: Lookup = new Map();
    for (const [version, run] of kept) {
        const inner = new Map<string, number>();
        for (const p of run.points) {
            if (p.coord.tool !== OMNI || !isUsable(p)) continue;
            const scenario = canonicalScenario(p.coord.scenario, aliases);
            inner.set(`${scenario}|${p.coord.warmth}|${p.metric}`, p.median);
        }
        lookup.set(version, inner);
    }
    return lookup;
}

function buildChart(
    metric: Metric,
    warmth: Warmth,
    versions: string[],
    scenarios: string[],
    lookup: Lookup,
): ChartSpec | null {
    if (versions.length === 0 || scenarios.length === 0) return null;

    const series: SeriesSpec[] = scenarios.map((scenario) => ({
        key: scenario,
        label: scenario,
        points: versions.map((version) => {
            const median = lookup
                .get(version)
                ?.get(`${scenario}|${warmth}|${metric}`);
            return { x: version, y: median ?? null };
        }),
    }));

    if (!series.some((s) => s.points.some((pt) => pt.y !== null))) return null;

    return {
        id: `version-history--${metric}--${warmth}`,
        kind: "line",
        title: `omni ${warmth} ${metricLabel(metric)} over versions`,
        x: { label: "version" },
        y: metricAxis(metric),
        series,
    };
}

function provenanceFor(
    version: string,
    run: NormalizedRun | undefined,
): BuildProvenance {
    const p: BuildProvenance = { version };
    if (run?.commitSha !== undefined) p.commitSha = run.commitSha;
    if (run?.sourceUrl !== undefined) p.sourceUrl = run.sourceUrl;
    if (run?.generatedAt !== undefined) p.generatedAt = run.generatedAt;
    return p;
}

function toExcludedItem(v: VersionVerdict): ExcludedItem {
    const failed = (Object.keys(v.checks) as MinDataCheck[]).filter(
        (c) => !v.checks[c],
    );
    return {
        label: `omni ${v.version}`,
        failed,
        reason: v.reason ?? "does not meet the minimum data requirements",
    };
}

function reasonFor(
    checks: Record<MinDataCheck, boolean>,
    policy: MinimumDataPolicy,
): string {
    const parts: string[] = [];
    if (!checks["linux-present"]) parts.push(`no ${policy.os} run`);
    else if (!checks["full-preset"])
        parts.push(`not the "${policy.preset}" preset`);
    if (policy.requireResources && !checks["resource-runs"]) {
        parts.push("resource runs missing");
    }
    return parts.join("; ") || "insufficient data";
}
