import { renderTable } from "../bench/format";
import { CACHE_HIT_THRESHOLD, cacheHitRatio } from "../bench/metrics";
import { renderReport, renderToolInfo } from "../bench/report";
import { formatBytes, formatMs } from "../bench/stats";
import { TOOLS, type Tool } from "../config";
import type { ToolInfo } from "../tools";
import type { SuiteResult, SuiteScenarioResult } from "./index";

/** Tools that appear in at least one scenario, in canonical order. */
function toolsInSuite(suite: SuiteResult): Tool[] {
    const present = new Set<Tool>();
    for (const s of suite.scenarios) {
        for (const t of s.result.tools) present.add(t.tool);
    }
    return TOOLS.filter((t) => present.has(t));
}

/** Pick each tool's info from the first scenario that reports it. */
function suiteToolInfo(suite: SuiteResult, tools: Tool[]): ToolInfo[] {
    const infos: ToolInfo[] = [];
    for (const tool of tools) {
        const info = suite.scenarios
            .flatMap((s) => s.result.toolInfo ?? [])
            .find((i) => i.tool === tool);
        if (info) infos.push(info);
    }
    return infos;
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

/** Median peak RSS for a tool's cold/warm scenario (or a placeholder). */
function memCell(
    scenario: SuiteScenarioResult,
    tool: Tool,
    phase: "cold" | "warm",
): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    const r = t[phase].resources;
    return r ? formatBytes(r.peakRssBytes.median) : "—";
}

/** Median CPU time + average parallelism for a tool's cold/warm scenario. */
function cpuCell(
    scenario: SuiteScenarioResult,
    tool: Tool,
    phase: "cold" | "warm",
): string {
    const t = scenario.result.tools.find((r) => r.tool === tool);
    if (!t || t.error) return t?.error ? "err" : "—";
    const r = t[phase].resources;
    if (!r) return "—";
    return `${formatMs(r.cpuTimeMs.median)} (${r.parallelism.median.toFixed(1)}×)`;
}

/** Whether any tool in any scenario carries measured resource stats. */
function suiteHasResources(suite: SuiteResult): boolean {
    return suite.scenarios.some((s) =>
        s.result.tools.some(
            (t) =>
                t.warm.resources !== undefined ||
                t.cold.resources !== undefined,
        ),
    );
}

/**
 * Render a full suite as Markdown: warm + cold median wall-time matrices, then
 * (when measured) warm + cold memory and CPU matrices, followed by the detailed
 * per-scenario report tables.
 */
export function renderSuiteMarkdown(suite: SuiteResult): string[] {
    const tools = toolsInSuite(suite);
    const lines: string[] = [];

    lines.push(`# ${suite.displayName}`);
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

    const toolInfo = suiteToolInfo(suite, tools);
    if (toolInfo.length > 0) {
        lines.push("## Tool info");
        lines.push("");
        lines.push(...renderToolInfo(toolInfo));
        lines.push("");
    }
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
                s.displayName,
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
                s.displayName,
                String(s.result.tools[0]?.taskGraphSize ?? 0),
                String(s.result.concurrency),
                ...tools.map((t) => coldCell(s, t)),
            ]),
        ),
    );
    lines.push("");

    // Resource summaries (only when at least one scenario measured them).
    if (suiteHasResources(suite)) {
        lines.push(
            "`mem` = median peak RSS of the tool + daemon (a sampled lower " +
                "bound). `cpu` = median CPU time with average parallelism " +
                "(`cpu-time / wall-time`).",
        );
        lines.push("");

        const resourceMatrix = (
            cell: (s: SuiteScenarioResult, t: Tool) => string,
        ): string[] =>
            renderTable(
                ["scenario", "nodes", "conc", ...tools],
                suite.scenarios.map((s) => [
                    s.displayName,
                    String(s.result.tools[0]?.taskGraphSize ?? 0),
                    String(s.result.concurrency),
                    ...tools.map((t) => cell(s, t)),
                ]),
            );

        lines.push("## Summary — warm memory (median peak RSS)");
        lines.push("");
        lines.push(...resourceMatrix((s, t) => memCell(s, t, "warm")));
        lines.push("");

        lines.push("## Summary — warm CPU (median CPU-time · parallelism)");
        lines.push("");
        lines.push(...resourceMatrix((s, t) => cpuCell(s, t, "warm")));
        lines.push("");

        lines.push("## Summary — cold memory (median peak RSS)");
        lines.push("");
        lines.push(...resourceMatrix((s, t) => memCell(s, t, "cold")));
        lines.push("");

        lines.push("## Summary — cold CPU (median CPU-time · parallelism)");
        lines.push("");
        lines.push(...resourceMatrix((s, t) => cpuCell(s, t, "cold")));
        lines.push("");
    }

    // Details.
    lines.push("## Details");
    lines.push("");
    for (const s of suite.scenarios) {
        lines.push(`### ${s.displayName}`);
        if (s.description) {
            lines.push(`> ${s.description}`);
        }
        lines.push(
            `* Config: ${s.config.projects} projects × ${s.config.tasksPerProject} tasks, ` +
                `strategy \`${s.config.dependency.strategy}\`.`,
        );
        lines.push(...renderReport(s.result, { includeToolInfo: false }));
        lines.push("");
    }

    return lines;
}
