import type { RunRef } from "../sources/types";
import type {
    Coord,
    Metric,
    NormalizedRun,
    PlatformInfo,
    SamplePoint,
    Tool,
    ToolInfo,
    Warmth,
} from "./model";
import type { ParsedSuite } from "./schema";

/**
 * Flatten a task-bench `SuiteResult` (validated as {@link ParsedSuite}) into a
 * {@link NormalizedRun}: one artifact, a flat table of {@link SamplePoint}s.
 *
 * This is the dashboard's coupling seam — the only place, alongside
 * `schema.ts`, that understands task-bench's shape. Raggedness (missing tools,
 * errored tools, absent resources, empty stats) becomes data + warnings, never
 * a thrown error. See DESIGN.md §5.2.
 */

type ParsedScenario = ParsedSuite["scenarios"][number];
type ParsedScenarioResult = ParsedScenario["result"]["tools"][number]["cold"];

const WARMTHS: Warmth[] = ["cold", "warm"];

export function normalize(
    ref: RunRef,
    suite: ParsedSuite,
    sourceId = "unknown",
): NormalizedRun[] {
    const warnings: string[] = [];
    const points: SamplePoint[] = [];

    // Artifact-level attributes come from the first scenario's result (all
    // scenarios in one artifact run on the same machine).
    const first = suite.scenarios[0]?.result;
    const os = first?.platform.os.platform ?? "unknown";
    const concurrency = first?.concurrency ?? 0;
    const daemon = first?.daemon ?? false;
    const platform: PlatformInfo = {
        cpus: first?.platform.cpus ?? [],
        memory: first?.platform.memory ?? { totalBytes: 0, freeBytes: 0 },
        os: first?.platform.os ?? { platform: os, release: "", arch: "" },
    };
    const toolInfo: ToolInfo[] = (first?.toolInfo ?? []).map((t) => ({
        ...t,
        tool: t.tool as Tool,
    }));

    if (suite.scenarios.length === 0) {
        warnings.push("artifact contains no scenarios");
    }

    for (const scenario of suite.scenarios) {
        collectScenario(ref, suite, scenario, points, warnings);
    }

    const run: NormalizedRun = {
        source: sourceId,
        version: ref.version,
        target: ref.target,
        os,
        preset: suite.name,
        generatedAt: suite.generatedAt,
        taskBenchVersion: suite.taskBenchVersion,
        concurrency,
        daemon,
        platform,
        toolInfo,
        points,
        warnings,
    };
    if (ref.commitSha !== undefined) run.commitSha = ref.commitSha;
    if (ref.sourceUrl !== undefined) run.sourceUrl = ref.sourceUrl;

    return [run];
}

function collectScenario(
    ref: RunRef,
    suite: ParsedSuite,
    scenario: ParsedScenario,
    points: SamplePoint[],
    warnings: string[],
): void {
    const result = scenario.result;
    const os = result.platform.os.platform;

    for (const toolResult of result.tools) {
        const tool = toolResult.tool as Tool;
        const toolVersion = result.versions[tool] ?? null;
        const errored = toolResult.error !== undefined;
        if (errored) {
            warnings.push(
                `${tool} errored in scenario "${scenario.name}": ${toolResult.error}`,
            );
        }

        for (const warmth of WARMTHS) {
            const sr = toolResult[warmth];
            const hasResources = sr.resources !== undefined;
            const coord: Coord = {
                version: ref.version,
                target: ref.target,
                os,
                preset: suite.name,
                scenario: scenario.name,
                tool,
                toolVersion,
                warmth,
            };

            points.push(
                statPoint(coord, "durationMs", sr.stats, hasResources, errored),
            );
            if (sr.resources) {
                points.push(
                    statPoint(
                        coord,
                        "peakRssBytes",
                        sr.resources.peakRssBytes,
                        true,
                        errored,
                    ),
                );
                points.push(
                    statPoint(
                        coord,
                        "cpuTimeMs",
                        sr.resources.cpuTimeMs,
                        true,
                        errored,
                    ),
                );
                points.push(
                    statPoint(
                        coord,
                        "parallelism",
                        sr.resources.parallelism,
                        true,
                        errored,
                    ),
                );
            }

            if (sr.stats.samples.length === 0) {
                warnings.push(
                    `${tool} ${warmth} in scenario "${scenario.name}" has no samples`,
                );
            }
        }
    }
}

function statPoint(
    coord: Coord,
    metric: Metric,
    stats: ParsedScenarioResult["stats"],
    hasResources: boolean,
    errored: boolean,
): SamplePoint {
    return {
        coord,
        metric,
        median: stats.median,
        mean: stats.mean,
        min: stats.min,
        max: stats.max,
        stddev: stats.stddev,
        n: stats.samples.length,
        hasResources,
        errored,
    };
}
