import { describe, expect, it } from "vitest";
import { computeStats, formatMs, median } from "./stats";

describe("median", () => {
    it("returns 0 for an empty set", () => {
        expect(median([])).toBe(0);
    });

    it("handles odd and even counts and is order-independent", () => {
        expect(median([3, 1, 2])).toBe(2);
        expect(median([4, 1, 3, 2])).toBe(2.5);
    });
});

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
