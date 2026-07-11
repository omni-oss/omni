import { describe, expect, it } from "vitest";
import type { ChartSpec, Report } from "../chart/ir";
import { makeRun } from "../testing/run-factory";
import { analyzeChart, annotateReport } from "./analysis";
import { crossTool } from "./cross-tool";
import { versionHistory } from "./version";

function bar(series: ChartSpec["series"]): ChartSpec {
    return {
        id: "c",
        kind: "grouped-bar",
        title: "Warm duration",
        x: { label: "scenario" },
        y: { label: "duration", unit: "ms" },
        series,
    };
}

describe("analyzeChart", () => {
    it("ranks tools and highlights omni when it loses", () => {
        const text = analyzeChart(
            bar([
                {
                    key: "omni",
                    label: "omni",
                    emphasis: true,
                    points: [{ x: "a", y: 700 }],
                },
                { key: "turbo", label: "turbo", points: [{ x: "a", y: 500 }] },
            ]),
        );
        expect(text).toContain("**turbo**");
        expect(text).toContain("**omni**");
        expect(text).toMatch(/~40% higher/);
    });

    it("reports omni leading when it has the lowest value", () => {
        const text = analyzeChart(
            bar([
                {
                    key: "omni",
                    label: "omni",
                    emphasis: true,
                    points: [{ x: "a", y: 400 }],
                },
                { key: "turbo", label: "turbo", points: [{ x: "a", y: 500 }] },
            ]),
        );
        expect(text).toContain("**omni** had the lowest duration");
    });

    it("describes an improving trend for line charts", () => {
        const text = analyzeChart({
            id: "l",
            kind: "line",
            title: "omni warm duration over versions",
            x: { label: "version" },
            y: { label: "duration", unit: "ms" },
            series: [
                {
                    key: "scale-300",
                    label: "scale-300",
                    points: [
                        { x: "0.3.0", y: 900 },
                        { x: "0.4.0", y: 800 },
                    ],
                },
            ],
        });
        expect(text).toMatch(/improved ~11%/);
        expect(text).toContain("`0.3.0`");
        expect(text).toContain("`0.4.0`");
    });

    it("returns undefined when a chart has no data", () => {
        expect(
            analyzeChart(
                bar([
                    {
                        key: "omni",
                        label: "omni",
                        points: [{ x: "a", y: null }],
                    },
                ]),
            ),
        ).toBeUndefined();
    });
});

describe("annotateReport", () => {
    it("fills chart + overall analysis on a composed report", () => {
        const run = makeRun({
            version: "0.4.0",
            os: "linux",
            preset: "full",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [
                        {
                            tool: "omni",
                            warm: 700,
                            cold: 8000,
                            resources: true,
                        },
                        { tool: "turbo", warm: 500, cold: 6000 },
                    ],
                },
            ],
        });
        const view = crossTool([run], { version: "0.4.0" });
        const report: Report = {
            title: "r",
            generatedAt: "t",
            meta: { spotlight: "0.4.0" },
            views: [view, versionHistory([run])],
        };
        annotateReport(report);

        expect(report.analysis).toContain("**omni 0.4.0**");
        expect(report.analysis).toMatch(/lowest median in \*\*\d+\/\d+\*\*/);
        expect(view.charts.every((c) => typeof c.analysis === "string")).toBe(
            true,
        );
    });
});
