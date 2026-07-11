import { describe, expect, it } from "vitest";
import { composeReport } from "./pipeline";
import { makeRun } from "./testing/run-factory";

describe("composeReport", () => {
    const eligible = makeRun({
        version: "0.4.0",
        os: "linux",
        preset: "full",
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    {
                        tool: "omni",
                        toolVersion: "0.4.0",
                        warm: 700,
                        cold: 8000,
                        resources: true,
                    },
                    { tool: "turbo", warm: 500, cold: 6000 },
                ],
            },
        ],
    });

    it("composes all views into a single report", () => {
        const report = composeReport([eligible], { sourceId: "test" });
        const kinds = report.views.map((v) => v.kind);
        expect(kinds).toContain("cross-tool");
        expect(kinds).toContain("version-history");
        expect(report.meta.source).toBe("test");
        expect(report.meta.spotlight).toBe("0.4.0");
    });

    it("defaults the spotlight to the latest version present", () => {
        const older = makeRun({
            version: "0.3.0",
            os: "linux",
            preset: "full",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [
                        {
                            tool: "omni",
                            warm: 900,
                            cold: 9000,
                            resources: true,
                        },
                    ],
                },
            ],
        });
        const report = composeReport([older, eligible]);
        expect(report.meta.spotlight).toBe("0.4.0");
    });

    it("can narrow to a single view", () => {
        const report = composeReport([eligible], { views: ["cross-tool"] });
        expect(report.views.map((v) => v.kind)).toEqual(["cross-tool"]);
    });

    it("omits empty views but keeps a version-history that only has exclusions", () => {
        // A single non-eligible run: cross-tool still renders (has charts),
        // version-history has no charts but must survive via its exclusion panel.
        const ineligible = makeRun({
            version: "0.2.0",
            os: "linux",
            preset: "quick", // fails the full-preset gate
            scenarios: [
                {
                    name: "scale-300",
                    tools: [{ tool: "omni", warm: 950, cold: 9500 }],
                },
            ],
        });
        const report = composeReport([ineligible]);
        const versionView = report.views.find(
            (v) => v.kind === "version-history",
        );
        expect(versionView?.charts).toHaveLength(0);
        expect(versionView?.exclusionPanel).toBeDefined();
    });

    it("reports a note when nothing matches", () => {
        const report = composeReport([], { views: ["cross-tool"] });
        expect(report.views).toHaveLength(0);
        expect(report.notes?.[0]).toMatch(/No runs matched/);
    });
});
