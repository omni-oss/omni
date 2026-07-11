import { describe, expect, it } from "vitest";
import type { Report } from "../chart/ir";
import { JsonRenderer } from "./json";

const REPORT: Report = {
    title: "omni performance comparison",
    generatedAt: "2026-05-01T12:00:00.000Z",
    meta: { source: "local-fs" },
    views: [
        {
            id: "cross-tool",
            kind: "cross-tool",
            title: "Cross-tool",
            generatedAt: "2026-05-01T12:00:00.000Z",
            meta: {},
            charts: [
                {
                    id: "warm-duration",
                    kind: "grouped-bar",
                    title: "Warm duration",
                    x: { label: "scenario" },
                    y: { label: "duration", unit: "ms" },
                    series: [
                        {
                            key: "omni",
                            label: "omni",
                            emphasis: true,
                            points: [{ x: "scale-300", y: 715, yError: 8.5 }],
                        },
                    ],
                },
            ],
        },
    ],
};

describe("JsonRenderer", () => {
    it("emits one JSON file containing the whole report", async () => {
        const out = await new JsonRenderer().render(REPORT);

        expect(out.files).toHaveLength(1);
        const [file] = out.files;
        if (!file) throw new Error("expected a rendered file");
        expect(file.path).toBe("report.json");
        expect(file.mime).toBe("application/json");

        const parsed = JSON.parse(file.content as string) as Report;
        expect(parsed).toEqual(REPORT);
        expect(parsed.views[0]?.charts[0]?.series[0]?.emphasis).toBe(true);
    });

    it("respects fileName and indent options", async () => {
        const out = await new JsonRenderer({
            fileName: "dashboard.json",
            indent: 0,
        }).render(REPORT);
        expect(out.files[0]?.path).toBe("dashboard.json");
    });
});
