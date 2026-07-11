import { type AiConfig, annotateReportWithAi } from "./analyze/ai";
import { annotateReport } from "./analyze/analysis";
import { crossTool } from "./analyze/cross-tool";
import {
    type PresetAliasMap,
    resolvePresetAliases,
} from "./analyze/preset-aliases";
import {
    resolveScenarioAliases,
    type ScenarioAliasMap,
} from "./analyze/scenario-aliases";
import { latestVersion } from "./analyze/select";
import {
    DEFAULT_MIN_DATA,
    type MinimumDataPolicy,
    versionHistory,
} from "./analyze/version";
import type { Dashboard, Report } from "./chart/ir";
import type { NormalizedRun } from "./ingest/model";
import { normalize } from "./ingest/normalize";
import { parseSuite } from "./ingest/schema";
import type { Renderer, RenderOutput } from "./render/types";
import type { DataSource, TargetId } from "./sources/types";

/**
 * Wires source → normalize → analyze → Chart IR → render. A single run fans out
 * over the analyzers and composes ALL applicable views into one {@link Report}
 * (one artifact, every view). See DESIGN.md §9.
 */

export type ViewKind = "cross-tool" | "version-history";

export interface ComposeOptions {
    /** Which views to build. Omitted ⇒ all views the data supports. */
    views?: ViewKind[];
    /** Spotlighted omni version for cross-tool. Defaults to the latest present. */
    version?: string;
    /** Restrict cross-tool to these targets. */
    targets?: TargetId[];
    /** Version-history minimum-data gate. */
    minData?: MinimumDataPolicy;
    /** Version-history scenario rename bridging. */
    scenarioAliases?: ScenarioAliasMap;
    /** Preset name canonicalization (for the minimum-data gate). */
    presetAliases?: PresetAliasMap;
    /** Recorded in report meta. */
    sourceId?: string;
}

const ALL_VIEWS: ViewKind[] = ["cross-tool", "version-history"];

/** Build the full {@link Report} from already-normalized runs (pure, no IO). */
export function composeReport(
    runs: NormalizedRun[],
    opts: ComposeOptions = {},
): Report {
    const want = opts.views ?? ALL_VIEWS;
    const aliases = opts.scenarioAliases ?? resolveScenarioAliases();
    const presetAliases = opts.presetAliases ?? resolvePresetAliases();
    const version = opts.version ?? latestVersion(runs);

    const views: Dashboard[] = [];
    if (want.includes("cross-tool") && version) {
        views.push(
            crossTool(runs, {
                version,
                ...(opts.targets ? { targets: opts.targets } : {}),
            }),
        );
    }
    if (want.includes("version-history")) {
        views.push(
            versionHistory(
                runs,
                opts.minData ?? DEFAULT_MIN_DATA,
                aliases,
                presetAliases,
            ),
        );
    }

    // Keep only views that produced something to show.
    const kept = views.filter(
        (v) => v.charts.length > 0 || v.exclusionPanel !== undefined,
    );

    const report: Report = {
        title: "omni performance comparison",
        generatedAt: new Date().toISOString(),
        meta: {
            source: opts.sourceId ?? "unknown",
            spotlight: version ?? null,
            runs: runs.length,
        },
        views: kept,
    };
    if (kept.length === 0) {
        report.notes = ["No runs matched the given filters."];
    }
    annotateReport(report);
    return report;
}

export interface SourceReportOptions extends ComposeOptions {
    source: DataSource;
    /** Restrict which versions are fetched from the source. */
    fetchVersions?: string[];
    /** When set, generate AI analysis for each graph + the overall report. */
    ai?: AiConfig;
    /** Surface non-fatal warnings (e.g. AI request failures). */
    onWarn?: (message: string) => void;
}

export interface RunPipelineOptions extends SourceReportOptions {
    renderer: Renderer;
}

/** Fetch from a source, normalize, and compose the {@link Report} (no render). */
export async function buildReport(opts: SourceReportOptions): Promise<Report> {
    const refs = await opts.source.listRuns({
        ...(opts.fetchVersions ? { versions: opts.fetchVersions } : {}),
        ...(opts.targets ? { targets: opts.targets } : {}),
    });
    const raws = await Promise.all(refs.map((r) => opts.source.fetchRaw(r)));
    const runs = raws.flatMap(({ ref, json }) =>
        normalize(ref, parseSuite(json), opts.source.descriptor.id),
    );

    const report = composeReport(runs, {
        ...opts,
        sourceId: opts.source.descriptor.id,
    });
    if (opts.ai) {
        await annotateReportWithAi(report, opts.ai, {
            ...(opts.onWarn ? { onWarn: opts.onWarn } : {}),
        });
    }
    return report;
}

/** Full pipeline: fetch from a source, normalize, compose, and render. */
export async function run(opts: RunPipelineOptions): Promise<RenderOutput> {
    const report = await buildReport(opts);
    return opts.renderer.render(report);
}
