import type { Unit } from "./chart/ir";

/**
 * Shared value/axis formatting used by analyzers and renderers. Neutral leaf
 * module (depends only on the Chart IR types).
 *
 * NOTE: `render/html/assets/client.js` intentionally keeps its own copy of this
 * logic — it is injected into the page as raw text and runs in the browser, so
 * it cannot import from here. Keep the two in sync.
 */

/** Human-readable formatting of a metric value for a given unit. */
export function formatValue(value: number | null, unit?: Unit): string {
    if (value === null) return "—";
    switch (unit) {
        case "ms":
            return value >= 1000
                ? `${(value / 1000).toFixed(2)}s`
                : `${Math.round(value)}ms`;
        case "s":
            return `${value.toFixed(2)}s`;
        case "bytes":
            return formatBytes(value);
        case "cores":
            return `${value.toFixed(2)}×`;
        case "%":
            return `${value.toFixed(1)}%`;
        default:
            return Number.isInteger(value) ? String(value) : value.toFixed(2);
    }
}

/** Base-1024 byte size. */
export function formatBytes(bytes: number): string {
    if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)}GB`;
    if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)}MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)}KB`;
    return `${Math.round(bytes)}B`;
}

/** Axis label including its unit, e.g. "duration (ms)". */
export function axisLabel(label: string, unit?: Unit): string {
    return unit ? `${label} (${unit})` : label;
}
