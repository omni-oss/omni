/** Summary statistics for a set of timing samples (all in milliseconds). */
export interface Stats {
    samples: number[];
    min: number;
    max: number;
    mean: number;
    median: number;
    stddev: number;
}

/** Median of a numeric sample set (0 for an empty set). */
export function median(values: number[]): number {
    if (values.length === 0) return 0;
    const sorted = [...values].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2
        : (sorted[mid] ?? 0);
}

export function computeStats(samples: number[]): Stats {
    if (samples.length === 0) {
        return { samples, min: 0, max: 0, mean: 0, median: 0, stddev: 0 };
    }
    const sorted = [...samples].sort((a, b) => a - b);
    const sum = sorted.reduce((acc, n) => acc + n, 0);
    const mean = sum / sorted.length;
    const variance =
        sorted.reduce((acc, n) => acc + (n - mean) ** 2, 0) / sorted.length;
    return {
        samples,
        min: sorted[0] ?? 0,
        max: sorted[sorted.length - 1] ?? 0,
        mean,
        median: median(sorted),
        stddev: Math.sqrt(variance),
    };
}

export function formatMs(ms: number): string {
    if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
    return `${ms.toFixed(0)}ms`;
}
