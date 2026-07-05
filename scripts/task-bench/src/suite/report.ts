import { formatReport } from "../bench/report";
import { formatMs } from "../bench/stats";
import { TOOLS, type Tool } from "../config";
import type { SuiteResult, SuiteScenarioResult } from "./index";

/** Tools that appear in at least one scenario, in canonical order. */
function toolsInSuite(suite: SuiteResult): Tool[] {
    const present = new Set<Tool>();
    for (const s of suite.scenarios) {
        for (const t of s.result.tools) present.add(t.tool);
    }
    return TOOLS.filter((t) => present.has(t));
}

/** One-line summary of the resolved tool versions used across the suite. */
function formatSuiteVersions(suite: SuiteResult, tools: Tool[]): string {
    const parts = tools.map((tool) => {
        const version = suite.scenarios
            .map((s) => s.result.versions[tool])
            .find((v) => v != null);
        return `\`${tool} ${version ?? "?"}\``;
    });
    return `Tool versions:\n\n${parts.join("\n\n")}`;
}

function warmCell(scenario: SuiteScenarioResult, tool: Tool): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    const hits =
        t.taskGraphSize > 0
            ? (t.taskGraphSize - t.warm.executedMedian) / t.taskGraphSize
            : 1;
    const value = formatMs(t.warm.stats.median);
    return hits < 0.995 ? `${value}⚠` : value;
}

function coldCell(scenario: SuiteScenarioResult, tool: Tool): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    return formatMs(t.cold.stats.median);
}

function table(headers: string[], rows: string[][]): string[] {
    const widths = headers.map((h, i) =>
        Math.max(h.length, ...rows.map((r) => (r[i] ?? "").length)),
    );
    const pad = (v: string, w: number) => v.padEnd(w);
    const line = (cells: string[]) =>
        `| ${cells.map((c, i) => pad(c, widths[i] ?? 0)).join(" | ")} |`;
    return [
        line(headers),
        `| ${widths.map((w) => "-".repeat(w)).join(" | ")} |`,
        ...rows.map(line),
    ];
}

/**
 * Render a full suite as Markdown: two summary matrices (warm + cold median per
 * tool per scenario) followed by the detailed per-scenario report tables.
 */
export function formatSuiteMarkdown(suite: SuiteResult): string {
    const tools = toolsInSuite(suite);
    const lines: string[] = [];

    lines.push(`# ${suite.name}`);
    lines.push("");
    lines.push(`TaskBench v${suite.taskBenchVersion}`);
    lines.push("");
    if (suite.description) {
        lines.push(suite.description);
        lines.push("");
    }
    lines.push(
        `Generated ${suite.generatedAt} · ${suite.scenarios.length} scenario(s).`,
    );
    lines.push("");
    lines.push(formatSuiteVersions(suite, tools));
    lines.push("");
    lines.push(
        "`warm` = median wall time with a verified 100% cache hit " +
            "(discovery + cache-restore overhead). `⚠` marks a scenario whose " +
            "warm runs were not fully cached. Absolute times are " +
            "hardware-dependent — read the ratios.",
    );
    lines.push("");

    // Warm summary.
    lines.push("## Summary — warm median");
    lines.push("");
    lines.push(
        ...table(
            ["scenario", "nodes", "conc", "daemon", ...tools],
            suite.scenarios.map((s) => [
                s.name,
                String(s.result.tools[0]?.taskGraphSize ?? 0),
                String(s.result.concurrency),
                s.result.daemon ? "on" : "off",
                ...tools.map((t) => warmCell(s, t)),
            ]),
        ),
    );
    lines.push("");

    // Cold summary.
    lines.push("## Summary — cold median");
    lines.push("");
    lines.push(
        ...table(
            ["scenario", "nodes", "conc", ...tools],
            suite.scenarios.map((s) => [
                s.name,
                String(s.result.tools[0]?.taskGraphSize ?? 0),
                String(s.result.concurrency),
                ...tools.map((t) => coldCell(s, t)),
            ]),
        ),
    );
    lines.push("");

    // Details.
    lines.push("## Details");
    lines.push("");
    for (const s of suite.scenarios) {
        const heading = s.description ? `${s.name} — ${s.description}` : s.name;
        lines.push(`### ${heading}`);
        lines.push(
            `Config: ${s.config.projects} projects × ${s.config.tasksPerProject} tasks, ` +
                `strategy \`${s.config.dependency.strategy}\`.`,
        );
        lines.push(formatReport(s.result).trimEnd());
        lines.push("");
    }

    return lines.join("\n");
}
