import semver from "semver";
import type { AxisSpec } from "../chart/ir";
import type { Metric, NormalizedRun, SamplePoint } from "../ingest/model";

/**
 * Shared filtering, grouping, and axis helpers for the analyzers. Pure and
 * domain-aware, but produces only generic Chart IR pieces. See DESIGN.md §6.
 */

export function unique<T>(items: Iterable<T>): T[] {
    return [...new Set(items)];
}

export function groupBy<T, K>(items: T[], key: (item: T) => K): Map<K, T[]> {
    const map = new Map<K, T[]>();
    for (const item of items) {
        const k = key(item);
        const bucket = map.get(k);
        if (bucket) bucket.push(item);
        else map.set(k, [item]);
    }
    return map;
}

/** A point is usable (chartable) when it has samples and its tool didn't error. */
export function isUsable(point: SamplePoint): boolean {
    return point.n > 0 && !point.errored;
}

/** Coerce to a comparable semver, or null when not version-like. */
function toSemver(version: string): string | null {
    return semver.valid(version) ?? semver.valid(semver.coerce(version) ?? "");
}

/**
 * Compare two version labels: valid semver ascending first, then any
 * non-semver labels (e.g. "dev", "local@…") alphabetically after them.
 */
export function compareVersions(a: string, b: string): number {
    const sa = toSemver(a);
    const sb = toSemver(b);
    if (sa && sb) return semver.compare(sa, sb);
    if (sa) return -1;
    if (sb) return 1;
    return a.localeCompare(b);
}

/** The highest valid-semver version present, else the last by comparison. */
export function latestVersion(runs: NormalizedRun[]): string | undefined {
    const versions = unique(runs.map((r) => r.version));
    if (versions.length === 0) return undefined;
    const semverVersions = versions.filter((v) => toSemver(v) !== null);
    const pool = semverVersions.length > 0 ? semverVersions : versions;
    return [...pool].sort(compareVersions).at(-1);
}

/** Generic axis descriptor for a metric (label + unit). */
export function metricAxis(metric: Metric): AxisSpec {
    switch (metric) {
        case "durationMs":
            return { label: "duration", unit: "ms" };
        case "peakRssBytes":
            return { label: "peak RSS", unit: "bytes" };
        case "cpuTimeMs":
            return { label: "CPU time", unit: "ms" };
        case "parallelism":
            return { label: "parallelism", unit: "cores" };
    }
}

export function metricLabel(metric: Metric): string {
    return metricAxis(metric).label;
}
