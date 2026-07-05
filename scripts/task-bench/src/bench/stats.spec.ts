import { describe, expect, it } from "vitest";
import { computeStats, formatMs } from "./stats";

describe("computeStats", () => {
    it("handles the empty case", () => {
        const stats = computeStats([]);
        expect(stats).toMatchObject({
            min: 0,
            max: 0,
            mean: 0,
            median: 0,
            stddev: 0,
        });
    });

    it("computes summary statistics", () => {
        const stats = computeStats([10, 20, 30]);
        expect(stats.min).toBe(10);
        expect(stats.max).toBe(30);
        expect(stats.mean).toBe(20);
        expect(stats.median).toBe(20);
        expect(stats.stddev).toBeCloseTo(Math.sqrt(200 / 3), 6);
    });

    it("averages the two middle values for an even count", () => {
        expect(computeStats([1, 2, 3, 4]).median).toBe(2.5);
    });
});

describe("formatMs", () => {
    it("uses ms below a second and seconds above", () => {
        expect(formatMs(250)).toBe("250ms");
        expect(formatMs(1500)).toBe("1.50s");
    });
});
