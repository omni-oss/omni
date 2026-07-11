import { describe, expect, it } from "vitest";
import { composeReport } from "../pipeline";
import { makeRun } from "../testing/run-factory";
import { MarkdownRenderer } from "./markdown";

function sampleReport() {
    const v040 = makeRun({
        version: "0.4.0",
        os: "linux",
        preset: "full",
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    { tool: "omni", warm: 800, cold: 8000, resources: true },
                    { tool: "turbo", warm: 600, cold: 6000 },
                ],
            },
        ],
    });
    const v030 = makeRun({
        version: "0.3.0",
        os: "linux",
        preset: "full",
        commitSha: "aaaaaaabbbbbb",
        sourceUrl: "https://github.com/o/r/tree/aaaaaaabbbbbb",
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    { tool: "omni", warm: 900, cold: 9000, resources: true },
                ],
            },
        ],
    });
    // A distinct, ineligible version (Windows-only) to force an exclusion panel.
    const win = makeRun({
        version: "0.2.0",
        os: "win32",
        target: "x86_64-pc-windows-msvc",
        preset: "full",
        scenarios: [
            { name: "scale-300", tools: [{ tool: "omni", warm: 1, cold: 2 }] },
        ],
    });
    return composeReport([v040, v030, win], {
        version: "0.4.0",
        sourceId: "test",
    });
}

describe("MarkdownRenderer", () => {
    it("emits one markdown file per report with a section per view", async () => {
        const out = await new MarkdownRenderer().render(sampleReport());
        expect(out.files).toHaveLength(1);
        const md = out.files[0]?.content as string;
        expect(out.files[0]?.path).toBe("report.md");
        expect(md).toContain("## omni 0.4.0 vs. other tools");
        expect(md).toContain("## omni performance across versions");
    });

    it("renders chart data tables with formatted values", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        // cross-tool warm duration table shows omni + turbo columns.
        expect(md).toMatch(/\| scenario \| omni \| turbo \|/);
        expect(md).toContain("800ms");
    });

    it("renders the mandatory exclusion panel as a callout table", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        expect(md).toContain("Excluded versions");
        expect(md).toContain("Requires: Linux · full preset · resource runs");
    });

    it("renders provenance with a view-source link", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        expect(md).toContain(
            "[aaaaaaa](https://github.com/o/r/tree/aaaaaaabbbbbb)",
        );
    });

    it("includes overall + per-chart analysis (Markdown)", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        // Overall analysis near the top.
        expect(md).toContain("Spotlighting **omni 0.4.0**");
        // At least one per-chart analysis sentence.
        expect(md).toMatch(/\*\*(omni|turbo)\*\*/);
    });

    it("renders collapsible tool + platform info groups", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        expect(md).toContain("<summary>Tools</summary>");
        expect(md).toContain("<summary>Platform</summary>");
        expect(md).toContain("host-binary");
    });

    it("prefers AI analysis (collapsible, labeled) over synthetic when present", async () => {
        const report = sampleReport();
        report.aiAnalysis = "**AI overall.**";
        const firstChart = report.views[0]?.charts[0];
        if (firstChart) firstChart.aiAnalysis = "AI per-graph note.";
        const md = (await new MarkdownRenderer().render(report)).files[0]
            ?.content as string;
        expect(md).toContain("<summary>AI analysis</summary>");
        expect(md).toContain("**AI overall.**");
        expect(md).toContain("AI per-graph note.");
    });

    it("emits a mermaid xychart for line (version-history) charts", async () => {
        const md = (await new MarkdownRenderer().render(sampleReport()))
            .files[0]?.content as string;
        expect(md).toContain("```mermaid");
        expect(md).toContain("xychart-beta");
    });
});
