import { describe, expect, it } from "vitest";
import type { ScenarioResult, ToolResult } from "./index";
import { cacheHitRatio, isFullyCached } from "./metrics";

function scenario(executedMedian: number, sampleCount = 3): ScenarioResult {
    return {
        runs: sampleCount,
        failures: 0,
        executedMedian,
        stats: {
            samples: Array.from({ length: sampleCount }, () => 1),
            min: 1,
            max: 1,
            mean: 1,
            median: 1,
            stddev: 0,
        },
    };
}

function tool(taskGraphSize: number, warmExecuted: number): ToolResult {
    return {
        tool: "omni",
        task: "t2",
        taskGraphSize,
        cold: scenario(taskGraphSize),
        warm: scenario(warmExecuted),
    };
}

describe("cacheHitRatio", () => {
    it("is 1 when nothing re-ran on warm", () => {
        expect(cacheHitRatio(tool(100, 0))).toBe(1);
    });

    it("reflects partial re-execution", () => {
        expect(cacheHitRatio(tool(100, 10))).toBeCloseTo(0.9, 6);
    });

    it("is null when the graph size is unknown", () => {
        expect(cacheHitRatio(tool(0, 0))).toBeNull();
    });
});

describe("isFullyCached", () => {
    it("accepts full/near-full hits and rejects partial ones", () => {
        expect(isFullyCached(tool(100, 0))).toBe(true);
        expect(isFullyCached(tool(1000, 1))).toBe(true); // 99.9% ≥ threshold
        expect(isFullyCached(tool(100, 5))).toBe(false); // 95%
        expect(isFullyCached(tool(0, 0))).toBe(false); // unknown
    });
});
