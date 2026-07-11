/**
 * The dashboard's output IR: a *generic visualization* structure. It is
 * deliberately decoupled from the benchmark domain — this module imports
 * nothing from `@omni-oss/task-bench` and nothing from `../ingest`. It knows
 * only charts, axes, series, and points. Analyzers flatten domain concepts
 * (tool, version, warm/cold, preset, RSS/CPU) into generic series + labels.
 *
 * See DESIGN.md §7.
 */

export type ChartKind = "bar" | "grouped-bar" | "line" | "scatter" | "table";
export type ScaleKind = "linear" | "log";
export type Unit = "ms" | "s" | "bytes" | "cores" | "count" | "%";

export interface AxisSpec {
    label: string;
    unit?: Unit;
    /** Defaults to "linear". */
    scale?: ScaleKind;
}

/** One point in a series. */
export interface Point {
    /** Category (scenario/version) or numeric x. */
    x: string | number;
    /** `null` ⇒ a genuine gap; renderers must not interpolate across it. */
    y: number | null;
    /** Optional error bar magnitude (e.g. from stddev). */
    yError?: number;
}

/** A named data series. */
export interface SeriesSpec {
    /** Stable id, e.g. a tool name or "omni@0.4.1". */
    key: string;
    label: string;
    /** Renderer hint: highlight this series (e.g. omni). */
    emphasis?: boolean;
    points: Point[];
}

/** A generic faceting tag; renderers may offer per-dimension filtering. */
export interface FacetTag {
    dimension: string;
    value: string;
}

export interface ChartSpec {
    id: string;
    kind: ChartKind;
    title: string;
    subtitle?: string;
    x: AxisSpec;
    y: AxisSpec;
    series: SeriesSpec[];
    /** Free-form, renderer-agnostic annotations (coverage notes, etc.). */
    notes?: string[];
    /** Short, data-derived analysis for this graph. May contain Markdown. */
    analysis?: string;
    /**
     * Optional AI-generated analysis for this graph. When present, renderers
     * show this (labeled as AI, collapsible) instead of {@link analysis}.
     */
    aiAnalysis?: string;
    /**
     * Faceting tags (e.g. `target`, `metric`, `warmth`). Renderers may group or
     * offer a per-dimension filter; purely a presentation hint.
     */
    facets?: FacetTag[];
}

/** One excluded unit (typically an omni version) and why. */
export interface ExcludedItem {
    label: string;
    /** One tag per failed check, for compact badges. */
    failed: string[];
    /** Full, human-readable explanation. */
    reason: string;
}

/**
 * A structured, MANDATORY-to-render panel: when present and non-empty, every
 * renderer must show it so an excluded version is always visible on screen.
 * See DESIGN.md §7.1.
 */
export interface ExclusionPanel {
    title: string;
    /** The requirement summary shown to the user. */
    criteria: string;
    items: ExcludedItem[];
}

/** One row per charted version: where its data came from. See DESIGN.md §7.2. */
export interface BuildProvenance {
    /** Matches a series key / x category. */
    version: string;
    commitSha?: string;
    sourceUrl?: string;
    /** Disambiguates the floating `main`/`latest`. */
    generatedAt?: string;
}

/**
 * A collapsible, tabular info panel (e.g. tool versions or platform specs).
 * Rendered like the per-chart data table: collapsed by default unless `open`.
 */
export interface InfoGroup {
    title: string;
    columns: string[];
    rows: string[][];
    open?: boolean;
}

/** One view within a report (produced by a single analyzer). */
export interface Dashboard {
    id: string;
    /** Open view discriminator, e.g. "cross-tool" | "version-history". */
    kind: string;
    title: string;
    description?: string;
    generatedAt: string;
    meta: Record<string, unknown>;
    charts: ChartSpec[];
    /** Free-form, renderer-agnostic annotations for the whole view (coverage, etc.). */
    notes?: string[];
    exclusionPanel?: ExclusionPanel;
    provenance?: BuildProvenance[];
    /** Collapsible context panels (tool info, platform specs, …). */
    info?: InfoGroup[];
}

/**
 * The top-level render unit: every view produced by a run, in one artifact.
 * A renderer consumes a whole Report and emits a single output.
 */
export interface Report {
    title: string;
    generatedAt: string;
    /** Global provenance: source id, filters, gate policy, etc. */
    meta: Record<string, unknown>;
    /** Short, data-derived overall analysis. May contain Markdown. */
    analysis?: string;
    /**
     * Optional AI-generated overall analysis. When present, renderers show this
     * (labeled as AI, collapsible) instead of {@link analysis}.
     */
    aiAnalysis?: string;
    /** All views, in display order. Empty views are omitted, never faked. */
    views: Dashboard[];
    /** Notes that apply report-wide. */
    notes?: string[];
}
