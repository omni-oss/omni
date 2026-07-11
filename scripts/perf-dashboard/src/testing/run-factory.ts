import type {
    Coord,
    Metric,
    NormalizedRun,
    PlatformInfo,
    SamplePoint,
    Tool,
    Warmth,
} from "../ingest/model";
import type { TargetId } from "../sources/types";

/**
 * Compact factory for {@link NormalizedRun}s in analyzer tests. Not part of the
 * published package (src/** is npmignored) and never exported from index.ts.
 */

/** Fixed, deterministic resource medians so tests can assert exact values. */
export const TEST_RESOURCE_MEDIAN: Record<
    "peakRssBytes" | "cpuTimeMs" | "parallelism",
    number
> = {
    peakRssBytes: 100,
    cpuTimeMs: 50,
    parallelism: 2,
};

export interface ToolSpec {
    tool: Tool;
    toolVersion?: string | null;
    /** Warm duration median; a number ⇒ usable point, null/undefined ⇒ gap. */
    warm?: number | null;
    /** Cold duration median; a number ⇒ usable point, null/undefined ⇒ gap. */
    cold?: number | null;
    /** Emit resource points for this tool's scenario. */
    resources?: boolean;
    /** Mark the tool as errored (all its points become unusable). */
    errored?: boolean;
}

export interface ScenarioSpec {
    name: string;
    tools: ToolSpec[];
}

export interface RunSpec {
    version: string;
    target?: TargetId;
    os?: string;
    preset?: string;
    generatedAt?: string;
    commitSha?: string;
    sourceUrl?: string;
    platform?: PlatformInfo;
    scenarios: ScenarioSpec[];
}

const WARMTHS: Warmth[] = ["warm", "cold"];
const RESOURCE_METRICS: Metric[] = ["peakRssBytes", "cpuTimeMs", "parallelism"];

export function makeRun(spec: RunSpec): NormalizedRun {
    const target = spec.target ?? "x86_64-unknown-linux-gnu";
    const os = spec.os ?? "linux";
    const preset = spec.preset ?? "full";
    const points: SamplePoint[] = [];

    for (const scenario of spec.scenarios) {
        for (const t of scenario.tools) {
            const hasResources = t.resources === true;
            for (const warmth of WARMTHS) {
                const median = warmth === "warm" ? t.warm : t.cold;
                const usable = typeof median === "number" && !t.errored;
                const coord: Coord = {
                    version: spec.version,
                    target,
                    os,
                    preset,
                    scenario: scenario.name,
                    tool: t.tool,
                    toolVersion: t.toolVersion ?? null,
                    warmth,
                };
                points.push(
                    point(
                        coord,
                        "durationMs",
                        median ?? 0,
                        usable,
                        hasResources,
                        t.errored === true,
                    ),
                );
                if (hasResources) {
                    for (const metric of RESOURCE_METRICS) {
                        points.push(
                            point(
                                coord,
                                metric,
                                TEST_RESOURCE_MEDIAN[
                                    metric as keyof typeof TEST_RESOURCE_MEDIAN
                                ],
                                usable,
                                true,
                                t.errored === true,
                            ),
                        );
                    }
                }
            }
        }
    }

    // Derive tool info from the distinct tools that appear in the scenarios.
    const toolInfo = [
        ...new Map(
            spec.scenarios
                .flatMap((s) => s.tools)
                .map((t) => [
                    t.tool,
                    {
                        tool: t.tool,
                        version: t.toolVersion ?? null,
                        daemon: false,
                        provisioning: "host-binary",
                        supportedVersions: [],
                        description: `${t.tool} adapter`,
                    },
                ]),
        ).values(),
    ];

    const run: NormalizedRun = {
        source: "test",
        version: spec.version,
        target,
        os,
        preset,
        generatedAt: spec.generatedAt ?? "2026-01-01T00:00:00.000Z",
        taskBenchVersion: "0.1.0",
        concurrency: 8,
        daemon: true,
        platform: spec.platform ?? {
            cpus: [{ model: "Test CPU", speedMHz: 2400 }],
            memory: { totalBytes: 16 * 1024 ** 3, freeBytes: 8 * 1024 ** 3 },
            os: { platform: os, release: "0.0", arch: "x64" },
        },
        toolInfo,
        points,
        warnings: [],
    };
    if (spec.commitSha !== undefined) run.commitSha = spec.commitSha;
    if (spec.sourceUrl !== undefined) run.sourceUrl = spec.sourceUrl;
    return run;
}

function point(
    coord: Coord,
    metric: Metric,
    median: number,
    usable: boolean,
    hasResources: boolean,
    errored: boolean,
): SamplePoint {
    return {
        coord,
        metric,
        median: usable ? median : 0,
        mean: usable ? median : 0,
        min: usable ? median : 0,
        max: usable ? median : 0,
        stddev: 0,
        n: usable ? 3 : 0,
        hasResources,
        errored,
    };
}
