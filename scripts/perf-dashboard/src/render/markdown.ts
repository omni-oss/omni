import type {
    BuildProvenance,
    ChartSpec,
    Dashboard,
    ExclusionPanel,
    InfoGroup,
    Report,
} from "../chart/ir";
import { axisLabel, formatValue } from "../format";
import type { Renderer, RenderOutput } from "./types";

/**
 * Renders a whole {@link Report} to a single Markdown document: one section per
 * view, each with data tables, a mermaid chart (for line views), a mandatory
 * exclusion panel, and a provenance table with "view source" links.
 * See DESIGN.md §8.
 */
export interface MarkdownRendererOptions {
    fileName?: string;
}

export class MarkdownRenderer implements Renderer {
    readonly id = "markdown";

    constructor(private readonly options: MarkdownRendererOptions = {}) {}

    render(report: Report): Promise<RenderOutput> {
        return Promise.resolve({
            files: [
                {
                    path: this.options.fileName ?? "report.md",
                    content: renderReport(report),
                    mime: "text/markdown",
                },
            ],
        });
    }
}

function renderReport(report: Report): string {
    const out: string[] = [];
    out.push(`# ${report.title}`, "");
    out.push(`_Generated ${report.generatedAt}._`, "");
    const overall = renderAnalysis(report.analysis, report.aiAnalysis, true);
    if (overall) out.push(overall, "");
    if (report.notes?.length) {
        for (const note of report.notes) out.push(`> ${note}`, "");
    }
    for (const view of report.views) {
        out.push(renderView(view));
    }
    return `${out.join("\n").trimEnd()}\n`;
}

function renderView(view: Dashboard): string {
    const out: string[] = [];
    out.push(`## ${view.title}`, "");
    if (view.description) out.push(view.description, "");
    if (view.notes?.length) {
        out.push(...view.notes.map((n) => `- ${n}`), "");
    }
    if (view.exclusionPanel) {
        out.push(renderExclusionPanel(view.exclusionPanel), "");
    }
    if (view.provenance?.length) {
        out.push(renderProvenance(view.provenance), "");
    }
    for (const group of view.info ?? []) {
        out.push(renderInfoGroup(group), "");
    }
    for (const chart of view.charts) {
        out.push(renderChart(chart));
    }
    return out.join("\n");
}

function renderInfoGroup(group: InfoGroup): string {
    // Collapsible on GitHub via <details>, with a markdown table inside.
    const out: string[] = [];
    out.push(`<details${group.open ? " open" : ""}>`);
    out.push(`<summary>${group.title}</summary>`, "");
    out.push(`| ${group.columns.join(" | ")} |`);
    out.push(`| ${group.columns.map(() => "---").join(" | ")} |`);
    for (const row of group.rows) {
        out.push(`| ${row.map((c) => c.replace(/\|/g, "\\|")).join(" | ")} |`);
    }
    out.push("", "</details>");
    return out.join("\n");
}

function renderExclusionPanel(panel: ExclusionPanel): string {
    const out: string[] = [];
    out.push(`> ### ${panel.title}`);
    out.push(`> ${panel.criteria}`);
    out.push(">");
    out.push("> | version | missing | reason |");
    out.push("> | --- | --- | --- |");
    for (const item of panel.items) {
        out.push(
            `> | ${item.label} | ${item.failed.join(", ") || "—"} | ${item.reason} |`,
        );
    }
    return out.join("\n");
}

function renderProvenance(provenance: BuildProvenance[]): string {
    const out: string[] = [];
    out.push("**Provenance**", "");
    out.push("| version | build | generated |");
    out.push("| --- | --- | --- |");
    for (const p of provenance) {
        const build = p.sourceUrl
            ? `[${shortSha(p.commitSha) ?? "source"}](${p.sourceUrl})`
            : (shortSha(p.commitSha) ?? "—");
        out.push(`| ${p.version} | ${build} | ${p.generatedAt ?? "—"} |`);
    }
    return out.join("\n");
}

/**
 * Analysis block. AI analysis wins and renders as a labeled collapsible
 * `<details>` (open for the report, collapsed for a graph); synthetic analysis
 * renders inline (short, not collapsible). Returns "" when neither is present.
 */
function renderAnalysis(
    analysis: string | undefined,
    aiAnalysis: string | undefined,
    open: boolean,
): string {
    if (aiAnalysis) {
        return [
            `<details${open ? " open" : ""}>`,
            "<summary>AI analysis</summary>",
            "",
            aiAnalysis,
            "",
            "</details>",
        ].join("\n");
    }
    return analysis ?? "";
}

function renderChart(chart: ChartSpec): string {
    const out: string[] = [];
    out.push(`### ${chart.title}`, "");
    if (chart.subtitle) out.push(`_${chart.subtitle}_`, "");
    const analysis = renderAnalysis(chart.analysis, chart.aiAnalysis, false);
    if (analysis) out.push(analysis, "");
    out.push(renderTable(chart), "");
    if (chart.notes?.length) {
        out.push(...chart.notes.map((n) => `- ${n}`), "");
    }
    const mermaid = renderMermaid(chart);
    if (mermaid) out.push(mermaid, "");
    return out.join("\n");
}

function renderTable(chart: ChartSpec): string {
    const categories = chart.series[0]?.points.map((p) => p.x) ?? [];
    const header = [chart.x.label, ...chart.series.map((s) => s.label)];
    const rows: string[] = [];
    rows.push(`| ${header.join(" | ")} |`);
    rows.push(`| ${header.map(() => "---").join(" | ")} |`);
    categories.forEach((x, i) => {
        const cells = chart.series.map((s) =>
            formatValue(s.points[i]?.y ?? null, chart.y.unit),
        );
        rows.push(`| ${x} | ${cells.join(" | ")} |`);
    });
    return rows.join("\n");
}

/**
 * A mermaid `xychart-beta` line chart for line views (version trends render
 * cleanly). Series with any gap (null) are omitted from the chart but stay in
 * the table above. Returns null when nothing is chartable.
 */
function renderMermaid(chart: ChartSpec): string | null {
    if (chart.kind !== "line") return null;
    const categories = chart.series[0]?.points.map((p) => String(p.x)) ?? [];
    if (categories.length === 0) return null;

    const lines = chart.series.filter((s) =>
        s.points.every((p) => p.y !== null),
    );
    if (lines.length === 0) return null;

    const out: string[] = ["```mermaid", "xychart-beta"];
    out.push(`    title "${chart.title}"`);
    out.push(`    x-axis [${categories.map(quote).join(", ")}]`);
    out.push(`    y-axis "${axisLabel(chart.y.label, chart.y.unit)}"`);
    for (const s of lines) {
        const values = s.points.map((p) => p.y as number);
        out.push(`    line [${values.join(", ")}]`);
    }
    out.push("```");
    return out.join("\n");
}

function quote(s: string): string {
    return `"${s.replace(/"/g, "'")}"`;
}

function shortSha(sha?: string): string | undefined {
    return sha ? sha.slice(0, 7) : undefined;
}
