import { describe, expect, it } from "vitest";
import type { Report } from "../chart/ir";
import { composeReport } from "../pipeline";
import { makeRun } from "../testing/run-factory";
import { HtmlRenderer } from "./html";

function sampleReport() {
    const eligible = makeRun({
        version: "0.4.0",
        os: "linux",
        preset: "full",
        commitSha: "deadbeef1234",
        sourceUrl: "https://github.com/o/r/tree/deadbeef1234",
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
    const ineligible = makeRun({
        version: "0.2.0",
        os: "linux",
        preset: "quick",
        scenarios: [
            {
                name: "scale-300",
                tools: [{ tool: "omni", warm: 950, cold: 9500 }],
            },
        ],
    });
    return composeReport([eligible, ineligible], {
        version: "0.4.0",
        sourceId: "test",
    });
}

describe("HtmlRenderer", () => {
    it("emits an index.html that loads the ECharts library", async () => {
        const out = await new HtmlRenderer().render(sampleReport());
        expect(out.files).toHaveLength(1);
        expect(out.files[0]?.path).toBe("index.html");
        expect(out.files[0]?.mime).toBe("text/html");
        const html = out.files[0]?.content as string;
        expect(html.startsWith("<!doctype html>")).toBe(true);
        expect(html).toMatch(
            /<script src="https:\/\/[^"]*echarts[^"]*"><\/script>/,
        );
    });

    it("emits a script tag for the marked library", async () => {
        const html = (await new HtmlRenderer().render(sampleReport())).files[0]
            ?.content as string;
        expect(html).toMatch(
            /<script src="https:\/\/[^"]*marked[^"]*"><\/script>/,
        );
    });

    it("honors a custom ECharts URL", async () => {
        const html = (
            await new HtmlRenderer({
                echartsUrl: "https://example.com/echarts.js",
            }).render(sampleReport())
        ).files[0]?.content as string;
        expect(html).toContain('<script src="https://example.com/echarts.js">');
    });

    it("embeds the report JSON and escapes `<` so it can't break the script", async () => {
        const report = sampleReport();
        const html = (await new HtmlRenderer().render(report)).files[0]
            ?.content as string;
        expect(html).toContain(
            '<script type="application/json" id="report-data">',
        );
        // Round-trip: the embedded JSON parses back to the same report.
        const start = html.indexOf(">", html.indexOf("report-data")) + 1;
        const end = html.indexOf("</script>", start);
        const embedded = html.slice(start, end).replace(/\\u003c/g, "<");
        expect(JSON.parse(embedded) as Report).toEqual(report);
    });

    it("renders a tab per view and the exclusion callout", async () => {
        const html = (await new HtmlRenderer().render(sampleReport())).files[0]
            ?.content as string;
        expect(html).toContain("omni 0.4.0 vs. other tools");
        expect(html).toContain("omni performance across versions");
        // Exclusion panel text (embedded in the JSON payload for hydration).
        expect(html).toContain("Excluded versions");
    });

    it("honors a custom marked URL", async () => {
        const html = (
            await new HtmlRenderer({
                markedUrl: "https://example.com/marked.js",
            }).render(sampleReport())
        ).files[0]?.content as string;
        expect(html).toContain('<script src="https://example.com/marked.js">');
    });

    it("embeds AI analysis and the AI-rendering client logic", async () => {
        const report = sampleReport();
        report.aiAnalysis = "AI overall marker";
        const html = (await new HtmlRenderer().render(report)).files[0]
            ?.content as string;
        // AI text is embedded (hydrated client-side into a collapsible block).
        expect(html).toContain("AI overall marker");
        // Client distinguishes AI vs synthetic analysis.
        expect(html).toContain("aiAnalysis");
        expect(html).toContain("<summary>AI analysis</summary>");
    });

    it("includes the provenance source URL", async () => {
        const html = (await new HtmlRenderer().render(sampleReport())).files[0]
            ?.content as string;
        expect(html).toContain("https://github.com/o/r/tree/deadbeef1234");
    });

    it("inlines the client script with no leftover template placeholders", async () => {
        const html = (await new HtmlRenderer().render(sampleReport())).files[0]
            ?.content as string;
        // Client script inlined (distinctive symbols from assets/client.js);
        // this also guards against template corruption of the {{CLIENT}} slot.
        expect(html).toContain("PALETTE");
        expect(html).toContain('getElementById("report-data")');
        // Target facet dropdown logic is present in the client.
        expect(html).toContain("facet-select");
        // Markdown analysis rendering is present (uses marked when CDN is available).
        expect(html).toContain("MARKED");
        expect(html).toContain("const md =");
        expect(html).toContain('class="analysis');
        // No unreplaced template placeholders remain.
        expect(html).not.toMatch(/\{\{[A-Z]+\}\}/);
        // NOTE: CSS inlining (`styles.css?raw`) is verified in the bun/Rollup
        // builds; vitest's CSS pipeline stubs `?raw` CSS to empty, so it isn't
        // asserted here.
    });
});
