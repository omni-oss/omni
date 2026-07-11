import { describe, expect, it } from "vitest";
import { makeRun } from "../testing/run-factory";
import { CONSTANT_PRESET_ALIASES } from "./preset-aliases";
import { DEFAULT_MIN_DATA, versionHistory } from "./version";

/** An eligible version: Linux + full preset + omni resource data. */
function eligible(
    version: string,
    warm: number,
    opts: { commitSha?: string } = {},
) {
    return makeRun({
        version,
        target: "x86_64-unknown-linux-gnu",
        os: "linux",
        preset: "full",
        ...(opts.commitSha ? { commitSha: opts.commitSha } : {}),
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    {
                        tool: "omni",
                        toolVersion: version,
                        warm,
                        cold: warm * 10,
                        resources: true,
                    },
                ],
            },
        ],
    });
}

describe("versionHistory", () => {
    it("canonicalizes the preset name via preset aliases before gating", () => {
        // task-bench serializes the `full` preset as "full suite".
        const run = makeRun({
            version: "0.4.0",
            os: "linux",
            preset: "full suite",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [
                        {
                            tool: "omni",
                            warm: 800,
                            cold: 8000,
                            resources: true,
                        },
                    ],
                },
            ],
        });

        // Without aliases the "full suite" preset fails the full-preset gate.
        const dropped = versionHistory([run], DEFAULT_MIN_DATA, {}, {});
        expect(dropped.meta.keptVersions).toEqual([]);
        expect(dropped.exclusionPanel?.items[0]?.failed).toContain(
            "full-preset",
        );

        // With the built-in preset aliases it canonicalizes to "full" → kept.
        const kept = versionHistory(
            [run],
            DEFAULT_MIN_DATA,
            {},
            CONSTANT_PRESET_ALIASES,
        );
        expect(kept.meta.keptVersions).toEqual(["0.4.0"]);
    });

    it("trends omni over eligible versions", () => {
        const runs = [eligible("0.3.0", 900), eligible("0.4.0", 800)];
        const view = versionHistory(runs, DEFAULT_MIN_DATA);

        const warmDuration = view.charts.find(
            (c) => c.id === "version-history--durationMs--warm",
        );
        expect(warmDuration?.kind).toBe("line");
        // versions are semver-sorted ascending on the x-axis.
        const scale = warmDuration?.series.find((s) => s.key === "scale-300");
        expect(scale?.points.map((p) => p.x)).toEqual(["0.3.0", "0.4.0"]);
        expect(scale?.points.map((p) => p.y)).toEqual([900, 800]);
    });

    it("emits resource trend charts (guaranteed by the gate)", () => {
        const runs = [eligible("0.3.0", 900), eligible("0.4.0", 800)];
        const view = versionHistory(runs, DEFAULT_MIN_DATA);
        expect(
            view.charts.some(
                (c) => c.id === "version-history--peakRssBytes--warm",
            ),
        ).toBe(true);
        expect(
            view.charts.some(
                (c) => c.id === "version-history--cpuTimeMs--cold",
            ),
        ).toBe(true);
    });

    it("drops versions missing resource data and reports them on screen", () => {
        const noResources = makeRun({
            version: "0.2.0",
            os: "linux",
            preset: "full",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [{ tool: "omni", warm: 950, cold: 9500 }],
                },
            ],
        });
        const view = versionHistory([noResources, eligible("0.4.0", 800)]);

        expect(view.meta.keptVersions).toEqual(["0.4.0"]);
        expect(view.exclusionPanel).toBeDefined();
        const item = view.exclusionPanel?.items.find(
            (i) => i.label === "omni 0.2.0",
        );
        expect(item?.failed).toContain("resource-runs");
        expect(item?.reason).toMatch(/resource runs missing/);
    });

    it("drops non-linux and non-full versions with specific reasons", () => {
        const windows = makeRun({
            version: "0.3.0",
            os: "win32",
            target: "x86_64-pc-windows-msvc",
            preset: "full",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [
                        { tool: "omni", warm: 1, cold: 2, resources: true },
                    ],
                },
            ],
        });
        const quick = makeRun({
            version: "0.3.5",
            os: "linux",
            preset: "quick",
            scenarios: [
                {
                    name: "scale-300",
                    tools: [
                        { tool: "omni", warm: 1, cold: 2, resources: true },
                    ],
                },
            ],
        });
        const view = versionHistory([windows, quick, eligible("0.4.0", 800)]);

        expect(view.meta.keptVersions).toEqual(["0.4.0"]);
        const win = view.exclusionPanel?.items.find(
            (i) => i.label === "omni 0.3.0",
        );
        expect(win?.failed).toContain("linux-present");
        const q = view.exclusionPanel?.items.find(
            (i) => i.label === "omni 0.3.5",
        );
        expect(q?.failed).toContain("full-preset");
    });

    it("only trends scenarios common to all kept versions and breaks the line on gaps", () => {
        const v1 = makeRun({
            version: "0.3.0",
            os: "linux",
            preset: "full",
            scenarios: [
                {
                    name: "shared",
                    tools: [
                        {
                            tool: "omni",
                            warm: 100,
                            cold: 1000,
                            resources: true,
                        },
                    ],
                },
                {
                    name: "only-in-v1",
                    tools: [
                        {
                            tool: "omni",
                            warm: 200,
                            cold: 2000,
                            resources: true,
                        },
                    ],
                },
            ],
        });
        const v2 = makeRun({
            version: "0.4.0",
            os: "linux",
            preset: "full",
            scenarios: [
                {
                    name: "shared",
                    tools: [
                        { tool: "omni", warm: 90, cold: 900, resources: true },
                    ],
                },
            ],
        });
        const view = versionHistory([v1, v2]);
        expect(view.meta.commonScenarios).toEqual(["shared"]);

        const warm = view.charts.find(
            (c) => c.id === "version-history--durationMs--warm",
        );
        expect(warm?.series.map((s) => s.key)).toEqual(["shared"]);
    });

    it("carries build provenance for kept versions", () => {
        const runs = [eligible("0.4.0", 800, { commitSha: "abc123" })];
        const view = versionHistory(runs);
        const prov = view.provenance?.find((p) => p.version === "0.4.0");
        expect(prov?.commitSha).toBe("abc123");
        expect(prov?.generatedAt).toBeDefined();
    });
});
