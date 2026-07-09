import { describe, expect, it } from "vitest";
import type { ToolInfo } from "../tools";
import type {
    BenchmarkResult,
    ResourceStats,
    ScenarioResult,
    ToolResult,
} from "./index";
import type { PlatformInfo } from "./platform-info";
import { renderReport, renderToolInfo } from "./report";
import type { Stats } from "./stats";

const info: ToolInfo = {
    tool: "turbo",
    version: "2.10.3",
    daemon: true,
    provisioning: "workspace-dependency",
    supportedVersions: ["^2.0.0"],
    description: "Vercel Turborepo. Runs a persistent daemon.",
};

describe("renderToolInfo", () => {
    it("renders version and the key attributes for each tool", () => {
        const text = renderToolInfo([info]).join("\n");
        expect(text).toContain("**turbo** 2.10.3");
        expect(text).toContain("daemon: yes");
        expect(text).toContain("provisioning: workspace-dependency");
        expect(text).toContain("supported: ^2.0.0");
        expect(text).toContain("Vercel Turborepo");
    });

    it("shows '?' for a missing version and 'no' for no daemon", () => {
        const text = renderToolInfo([
            { ...info, tool: "omni", version: null, daemon: false },
        ]).join("\n");
        expect(text).toContain("**omni** ?");
        expect(text).toContain("daemon: no");
    });

    it("returns nothing for an empty list", () => {
        expect(renderToolInfo([])).toEqual([]);
    });
});

const flatStats = (value: number): Stats => ({
    samples: [value, value, value],
    min: value,
    max: value,
    mean: value,
    median: value,
    stddev: 0,
});

const resourceStats: ResourceStats = {
    runs: 3,
    peakRssBytes: flatStats(1024 * 1024),
    cpuTimeMs: flatStats(500),
    parallelism: flatStats(2),
};

function scenario(
    executedMedian: number,
    resources?: ResourceStats,
): ScenarioResult {
    return {
        runs: 3,
        failures: 0,
        executedMedian,
        stats: flatStats(1),
        ...(resources ? { resources } : {}),
    };
}

const platform: PlatformInfo = {
    cpus: [{ model: "Test CPU", speedMHz: 3000 }],
    memory: {
        totalBytes: 8 * 1024 * 1024 * 1024,
        freeBytes: 4 * 1024 * 1024 * 1024,
    },
    os: { platform: "linux", release: "6.0", arch: "x64" },
};

function benchmark(tools: ToolResult[]): BenchmarkResult {
    return {
        rootDir: "/tmp/bench",
        task: "build",
        projects: 1,
        tasksPerProject: 1,
        concurrency: 1,
        daemon: false,
        versions: {},
        toolInfo: [],
        generatedAt: "2024-01-01T00:00:00.000Z",
        tools,
        platform,
    };
}

const RESOURCE_HEADERS = ["cold mem", "warm mem", "cold cpu", "warm cpu"];

describe("renderReport resource columns", () => {
    it("omits the resource columns when no tool reported usage", () => {
        const text = renderReport(
            benchmark([
                {
                    tool: "omni",
                    task: "build",
                    taskGraphSize: 1,
                    cold: scenario(1),
                    warm: scenario(0),
                },
            ]),
            { includeToolInfo: false },
        ).join("\n");
        for (const header of RESOURCE_HEADERS) {
            expect(text).not.toContain(header);
        }
        expect(text).toContain("warm cache-hit");
    });

    it("renders the resource columns when any tool reports usage, even if incomplete", () => {
        const text = renderReport(
            benchmark([
                {
                    tool: "omni",
                    task: "build",
                    taskGraphSize: 1,
                    cold: scenario(1, resourceStats),
                    warm: scenario(0, resourceStats),
                },
                {
                    // No resource data: its cells should be "—" but the
                    // columns still render because omni has data.
                    tool: "turbo",
                    task: "build",
                    taskGraphSize: 1,
                    cold: scenario(1),
                    warm: scenario(0),
                },
            ]),
            { includeToolInfo: false },
        ).join("\n");
        for (const header of RESOURCE_HEADERS) {
            expect(text).toContain(header);
        }
    });
});
