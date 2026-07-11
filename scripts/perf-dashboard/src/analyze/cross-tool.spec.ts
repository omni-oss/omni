import { describe, expect, it } from "vitest";
import type { ChartSpec } from "../chart/ir";
import { makeRun } from "../testing/run-factory";
import { crossTool } from "./cross-tool";

function findChart(charts: ChartSpec[], id: string): ChartSpec | undefined {
    return charts.find((c) => c.id === id);
}

describe("crossTool", () => {
    const run = makeRun({
        version: "0.4.1",
        target: "x86_64-unknown-linux-gnu",
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    {
                        tool: "omni",
                        toolVersion: "0.4.1",
                        warm: 715,
                        cold: 8650,
                        resources: true,
                    },
                    { tool: "turbo", warm: 523, cold: 6440 },
                    { tool: "nx", warm: null, cold: null, errored: true },
                ],
            },
        ],
    });

    it("emits warm & cold duration charts faceted by target/metric/warmth", () => {
        const view = crossTool([run], { version: "0.4.1" });
        const warm = findChart(
            view.charts,
            "x86_64-unknown-linux-gnu--durationMs--warm",
        );
        expect(warm).toBeDefined();
        expect(warm?.facets).toEqual(
            expect.arrayContaining([
                { dimension: "target", value: "x86_64-unknown-linux-gnu" },
                { dimension: "metric", value: "duration" },
                { dimension: "warmth", value: "warm" },
            ]),
        );
        expect(warm?.kind).toBe("grouped-bar");
    });

    it("spotlights omni (emphasis) and orders it first", () => {
        const view = crossTool([run], { version: "0.4.1" });
        const warm = findChart(
            view.charts,
            "x86_64-unknown-linux-gnu--durationMs--warm",
        );
        expect(warm?.series[0]?.key).toBe("omni");
        expect(warm?.series[0]?.emphasis).toBe(true);
    });

    it("omits the errored tool from the series", () => {
        const view = crossTool([run], { version: "0.4.1" });
        const warm = findChart(
            view.charts,
            "x86_64-unknown-linux-gnu--durationMs--warm",
        );
        const keys = warm?.series.map((s) => s.key) ?? [];
        expect(keys).toContain("omni");
        expect(keys).toContain("turbo");
        expect(keys).not.toContain("nx");
    });

    it("emits resource charts only because omni has resource data", () => {
        const view = crossTool([run], { version: "0.4.1" });
        expect(
            findChart(
                view.charts,
                "x86_64-unknown-linux-gnu--peakRssBytes--warm",
            ),
        ).toBeDefined();
        expect(
            findChart(view.charts, "x86_64-unknown-linux-gnu--cpuTimeMs--cold"),
        ).toBeDefined();
    });

    it("does not emit resource charts when omni lacks resource data", () => {
        const noRes = makeRun({
            version: "0.4.1",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [{ tool: "omni", warm: 715, cold: 8650 }],
                },
            ],
        });
        const view = crossTool([noRes], { version: "0.4.1" });
        expect(view.charts.every((c) => c.id.includes("durationMs"))).toBe(
            true,
        );
    });

    it("carries the spotlight version's data value", () => {
        const view = crossTool([run], { version: "0.4.1" });
        const warm = findChart(
            view.charts,
            "x86_64-unknown-linux-gnu--durationMs--warm",
        );
        const omni = warm?.series.find((s) => s.key === "omni");
        expect(omni?.points[0]?.y).toBe(715);
    });
});
