import { formatVersionList, renderTable } from "../bench/format";
import { CACHE_HIT_THRESHOLD, cacheHitRatio } from "../bench/metrics";
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
    const pairs = tools.map((tool) => {
        const version = suite.scenarios
            .map((s) => s.result.versions[tool])
            .find((v) => v != null);
        return [tool, version] as const;
    });
    return formatVersionList(pairs, "Tool versions");
}

function warmCell(scenario: SuiteScenarioResult, tool: Tool): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    const ratio = cacheHitRatio(t) ?? 1;
    const value = formatMs(t.warm.stats.median);
    return ratio < CACHE_HIT_THRESHOLD ? `${value}⚠` : value;
}

function coldCell(scenario: SuiteScenarioResult, tool: Tool): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    return formatMs(t.cold.stats.median);
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
        ...renderTable(
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
        ...renderTable(
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
        lines.push(`### ${s.name}`);
        if (s.description) {
            lines.push(`> ${s.description}`);
        }
        lines.push(
            `Config: ${s.config.projects} projects × ${s.config.tasksPerProject} tasks, ` +
                `strategy \`${s.config.dependency.strategy}\`.`,
        );
        lines.push(formatReport(s.result).trimEnd());
        lines.push("");
    }

    return lines.join("\n");
}
