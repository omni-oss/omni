import type { ToolResult } from "./index";

/** Warm runs at or above this cache-hit ratio are treated as fully cached. */
export const CACHE_HIT_THRESHOLD = 0.995;

/**
 * Fraction of the task graph served from cache on warm runs (1 == full hit),
 * or `null` when it can't be determined (no runs, or unknown graph size).
 */
export function cacheHitRatio(tool: ToolResult): number | null {
    if (tool.taskGraphSize <= 0 || tool.warm.stats.samples.length === 0) {
        return null;
    }
    return (tool.taskGraphSize - tool.warm.executedMedian) / tool.taskGraphSize;
}

/** Whether the tool's warm runs were (near-)perfectly cached. */
export function isFullyCached(tool: ToolResult): boolean {
    const ratio = cacheHitRatio(tool);
    return ratio !== null && ratio >= CACHE_HIT_THRESHOLD;
}
