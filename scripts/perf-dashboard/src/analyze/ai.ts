import type { ChartSpec, Report } from "../chart/ir";
import { formatValue } from "../format";
import { fetchWithRetry } from "../http";

/**
 * Optional, config-driven AI analysis. When enabled it generates a short
 * analysis per graph (from the labeled data points + the synthetic analysis)
 * and a longer overall analysis (from all per-graph context), stored in the
 * `aiAnalysis` fields. Config is resolved from env, then CLI overrides (which
 * win); if incomplete, AI is turned off with a warning naming the missing
 * value. Mirrors the workflow's AI step. See DESIGN.md §2.
 */

export type AiMode = "all-at-once" | "per-graph";

export interface AiConfig {
    baseUrl: string;
    model: string;
    apiKey: string;
    /**
     * How analyses are generated. `all-at-once` (default) makes a single
     * request for every graph + the overall report (avoids rate limits);
     * `per-graph` makes one request per graph plus one for the report.
     */
    mode?: AiMode;
    /** Max attempts for retryable failures (default 4). */
    maxAttempts?: number;
    /** Injectable fetch (for tests). Defaults to the global `fetch`. */
    fetchImpl?: typeof fetch;
}

export interface AiConfigInput {
    baseUrl?: string | undefined;
    model?: string | undefined;
    apiKey?: string | undefined;
    mode?: string | undefined;
}

export interface ResolvedAiConfig {
    /** The usable config, or null when AI is disabled. */
    config: AiConfig | null;
    /** Warnings to surface (e.g. which value was missing). */
    warnings: string[];
}

export interface AiRunOptions {
    onWarn?: (message: string) => void;
}

export const ENV_KEYS = {
    baseUrl: "AI_ANALYSIS_API_BASE_URL",
    model: "AI_ANALYSIS_MODEL",
    apiKey: "AI_ANALYSIS_API_KEY",
} as const;

export const FLAG_KEYS = {
    baseUrl: "--ai-analysis-api-base-url",
    model: "--ai-analysis-model",
    apiKey: "--ai-analysis-api-key",
} as const;

const LABELS = {
    baseUrl: "base URL",
    model: "model",
    apiKey: "API key",
} as const;

const MODE_ENV_KEY = "AI_ANALYSIS_MODE";
const MODE_FLAG_KEY = "--ai-analysis-mode";
const MODES: readonly AiMode[] = ["all-at-once", "per-graph"];
const DEFAULT_MODE: AiMode = "all-at-once";

type Field = keyof Omit<AiConfigInput, "mode">;

/**
 * Resolve AI config: env (`AI_ANALYSIS_*`) first, then CLI overrides (which
 * win). If none are set, AI is silently off. If some but not all are set, AI is
 * off and a warning names each missing value (env var + flag).
 */
export function resolveAiConfig(
    overrides: AiConfigInput = {},
    env: NodeJS.ProcessEnv = process.env,
): ResolvedAiConfig {
    const pick = (field: Field): string | undefined => {
        const override = overrides[field]?.trim();
        if (override) return override;
        const fromEnv = env[ENV_KEYS[field]]?.trim();
        return fromEnv ? fromEnv : undefined;
    };

    const baseUrl = pick("baseUrl");
    const model = pick("model");
    const apiKey = pick("apiKey");

    const anyProvided = [baseUrl, model, apiKey].some(Boolean);
    if (!anyProvided) return { config: null, warnings: [] };

    const missing: Field[] = [];
    if (!baseUrl) missing.push("baseUrl");
    if (!model) missing.push("model");
    if (!apiKey) missing.push("apiKey");

    if (missing.length > 0) {
        const warnings = missing.map(
            (f) =>
                `AI analysis: missing ${LABELS[f]} (set ${ENV_KEYS[f]} or ${FLAG_KEYS[f]}).`,
        );
        warnings.push("AI analysis disabled.");
        return { config: null, warnings };
    }

    const { mode, warnings } = resolveMode(overrides.mode, env);
    return {
        // biome-ignore lint/style/noNonNullAssertion: guarded by `missing` above.
        config: { baseUrl: baseUrl!, model: model!, apiKey: apiKey!, mode },
        warnings,
    };
}

/** Resolve the analysis mode from an override/env, defaulting to all-at-once. */
function resolveMode(
    override: string | undefined,
    env: NodeJS.ProcessEnv,
): { mode: AiMode; warnings: string[] } {
    const raw = override?.trim() || env[MODE_ENV_KEY]?.trim();
    if (!raw) return { mode: DEFAULT_MODE, warnings: [] };
    if ((MODES as readonly string[]).includes(raw)) {
        return { mode: raw as AiMode, warnings: [] };
    }
    return {
        mode: DEFAULT_MODE,
        warnings: [
            `AI analysis: unknown mode "${raw}" (set ${MODE_ENV_KEY} or ${MODE_FLAG_KEY} to ${MODES.join(" | ")}); using ${DEFAULT_MODE}.`,
        ],
    };
}

const CHART_SYSTEM_PROMPT =
    "You are a performance analyst reviewing ONE chart from a task-runner " +
    "benchmark (omni vs other tools, or omni across versions). You are given " +
    "the chart's labeled data points and a rule-based synthetic analysis. " +
    "Write a concise analysis. HARD LIMIT: at most 10 sentences, and as short " +
    "as possible. Be accurate, highlight the single most important comparison " +
    "or trend, and do not restate every number. Markdown is allowed, don't use any headings." +
    " Output is rendered as GFM: use \u2248 for approximations \u2014 single ~ creates strikethrough.";

const REPORT_SYSTEM_PROMPT =
    "You are a performance analyst summarizing a whole task-runner benchmark " +
    "report (omni vs other tools, and omni across its versions). You are given " +
    "the per-graph labeled data, synthetic analyses, and per-graph AI notes. " +
    "Write an overall analysis. It may be longer than a single graph's, but " +
    "stay focused and accurate, and call out the most important findings and " +
    "any caveats. Markdown is allowed, start with heading level 2 for the overall report." +
    " Output is rendered as GFM: use \u2248 for approximations \u2014 single ~ creates strikethrough.";

const BATCH_SYSTEM_PROMPT =
    "You are a performance analyst reviewing a whole task-runner benchmark " +
    "report (omni vs other tools, and omni across its versions). You are given " +
    "an overall synthetic analysis and a list of charts; each chart has a " +
    "unique KEY (e.g. g0), its labeled data points, and a synthetic analysis. " +
    "Respond with a SINGLE JSON object and NOTHING else (no code fences, no " +
    "prose), with exactly this shape:\n" +
    '{"report": string, "charts": {"<key>": string}}\n' +
    'Rules: "report" is the overall analysis (Markdown allowed; may be longer ' +
    "but focused and accurate; call out key findings and caveats, start with " +
    'heading level 2 for the overall report, don\'t use any headings for the charts). "charts" ' +
    "has ONE entry per provided KEY, using EXACTLY those keys; each value is " +
    "that chart's analysis in Markdown, at most 10 sentences and as short as " +
    "possible, highlighting the single most important comparison or trend " +
    "without restating every number." +
    " All Markdown is rendered as GFM: use \u2248 for approximations \u2014 single ~ creates strikethrough.";

/** Generate an AI analysis per graph and an overall one, in place. */
export async function annotateReportWithAi(
    report: Report,
    config: AiConfig,
    options: AiRunOptions = {},
): Promise<void> {
    const onWarn = options.onWarn ?? (() => {});
    const mode = config.mode ?? DEFAULT_MODE;
    if (mode === "per-graph") {
        await annotatePerGraph(report, config, onWarn);
    } else {
        await annotateAllAtOnce(report, config, onWarn);
    }
}

/** One request per graph + one for the report (simple, but N+1 requests). */
async function annotatePerGraph(
    report: Report,
    config: AiConfig,
    onWarn: (message: string) => void,
): Promise<void> {
    for (const view of report.views) {
        for (const chart of view.charts) {
            const text = await chat(
                config,
                CHART_SYSTEM_PROMPT,
                describeChart(chart),
                onWarn,
            );
            if (text) chart.aiAnalysis = text;
        }
    }

    const overall = await chat(
        config,
        REPORT_SYSTEM_PROMPT,
        describeReport(report),
        onWarn,
    );
    if (overall) report.aiAnalysis = overall;
}

/** A single request that returns keyed JSON for every graph + the report. */
async function annotateAllAtOnce(
    report: Report,
    config: AiConfig,
    onWarn: (message: string) => void,
): Promise<void> {
    const items = collectCharts(report);
    const content = await chat(
        config,
        BATCH_SYSTEM_PROMPT,
        describeReportBatch(report, items),
        onWarn,
    );
    if (!content) return;

    const parsed = parseBatch(content, onWarn);
    if (!parsed) return;

    if (typeof parsed.report === "string" && parsed.report.trim()) {
        report.aiAnalysis = parsed.report.trim();
    }
    const charts = parsed.charts;
    if (charts && typeof charts === "object") {
        const byKey = charts as Record<string, unknown>;
        for (const { key, chart } of items) {
            const value = byKey[key];
            if (typeof value === "string" && value.trim()) {
                chart.aiAnalysis = value.trim();
            }
        }
    }
}

interface ChartItem {
    key: string;
    chart: ChartSpec;
    viewTitle: string;
}

/** Assign a stable KEY (`g0`, `g1`, …) to every chart, in display order. */
function collectCharts(report: Report): ChartItem[] {
    const items: ChartItem[] = [];
    for (const view of report.views) {
        for (const chart of view.charts) {
            items.push({
                key: `g${items.length}`,
                chart,
                viewTitle: view.title,
            });
        }
    }
    return items;
}

/** The batch prompt body: overall context + every keyed chart. */
function describeReportBatch(report: Report, items: ChartItem[]): string {
    const parts: string[] = [`Report: ${report.title}`];
    if (report.analysis) {
        parts.push(`Overall synthetic analysis: ${report.analysis}`);
    }
    parts.push(
        "",
        `There are ${items.length} chart(s). Analyze each; its KEY is in brackets.`,
    );
    for (const { key, chart, viewTitle } of items) {
        parts.push(`\n[${key}] ${chart.title} (view: ${viewTitle})`);
        parts.push(describeChart(chart));
    }
    return parts.join("\n");
}

/** Parse the batch JSON response, tolerating code fences / surrounding prose. */
function parseBatch(
    content: string,
    onWarn: (message: string) => void,
): { report?: unknown; charts?: unknown } | null {
    let text = content.trim();
    const fenced = text.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/);
    if (fenced?.[1]) text = fenced[1].trim();
    if (!text.startsWith("{")) {
        const start = text.indexOf("{");
        const end = text.lastIndexOf("}");
        if (start >= 0 && end > start) text = text.slice(start, end + 1);
    }
    try {
        const obj = JSON.parse(text);
        if (obj && typeof obj === "object") {
            return obj as { report?: unknown; charts?: unknown };
        }
    } catch {
        // fall through
    }
    onWarn("AI analysis: could not parse JSON response; skipping.");
    return null;
}

/** Labeled data points + synthetic analysis for a single chart. */
function describeChart(chart: ChartSpec): string {
    const unit = chart.y.unit ? ` (${chart.y.unit})` : "";
    const lines: string[] = [];
    lines.push(`Chart: ${chart.title}`);
    lines.push(`Type: ${chart.kind}`);
    lines.push(`X axis: ${chart.x.label}; Y axis: ${chart.y.label}${unit}`);
    lines.push("Series (x=y; omni is the highlighted subject):");
    for (const s of chart.series) {
        const pts = s.points
            .map(
                (p) =>
                    `${p.x}=${p.y === null ? "n/a" : formatValue(p.y, chart.y.unit)}`,
            )
            .join(", ");
        lines.push(`- ${s.label}${s.emphasis ? " [omni]" : ""}: ${pts}`);
    }
    if (chart.analysis) lines.push(`Synthetic analysis: ${chart.analysis}`);
    return lines.join("\n");
}

/** All per-graph context (data + synthetic + AI notes) for the whole report. */
function describeReport(report: Report): string {
    const parts: string[] = [];
    parts.push(`Report: ${report.title}`);
    if (report.analysis) {
        parts.push(`Overall synthetic analysis: ${report.analysis}`);
    }
    for (const view of report.views) {
        parts.push(`\n## View: ${view.title}`);
        if (view.description) parts.push(view.description);
        for (const chart of view.charts) {
            parts.push(`\n${describeChart(chart)}`);
            if (chart.aiAnalysis) parts.push(`AI note: ${chart.aiAnalysis}`);
        }
    }
    return parts.join("\n");
}

/** POST a chat-completions request with retry/backoff; returns content or null. */
async function chat(
    config: AiConfig,
    system: string,
    user: string,
    onWarn: (message: string) => void,
): Promise<string | null> {
    const url = `${config.baseUrl.replace(/\/+$/, "")}/chat/completions`;
    const init: RequestInit = {
        method: "POST",
        headers: {
            Authorization: `Bearer ${config.apiKey}`,
            "Content-Type": "application/json",
        },
        body: JSON.stringify({
            model: config.model,
            temperature: 0.2,
            messages: [
                { role: "system", content: system },
                { role: "user", content: user },
            ],
        }),
    };

    try {
        const res = await fetchWithRetry(url, init, {
            ...(config.maxAttempts !== undefined
                ? { maxAttempts: config.maxAttempts }
                : {}),
            ...(config.fetchImpl ? { fetchImpl: config.fetchImpl } : {}),
        });
        if (!res.ok) {
            onWarn(`AI request failed (${res.status}): ${await res.text()}`);
            return null;
        }
        const data = (await res.json()) as {
            choices?: Array<{ message?: { content?: string } }>;
        };
        const content = data.choices?.[0]?.message?.content;
        return typeof content === "string" ? content.trim() : null;
    } catch (e) {
        onWarn(`AI request error: ${(e as Error).message}`);
        return null;
    }
}
