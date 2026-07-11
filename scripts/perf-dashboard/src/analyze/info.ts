import type { InfoGroup } from "../chart/ir";
import { formatBytes } from "../format";
import type { NormalizedRun, PlatformInfo, ToolInfo } from "../ingest/model";

/**
 * Builds the collapsible context panels (tool info + platform specs) shown
 * alongside a view's charts. Kept out of the renderers so every renderer gets
 * the same structured data. See DESIGN.md §7.
 */

/** One-line CPU summary, e.g. "8 × Intel(R) Core(TM) i7 @ 2400MHz". */
export function cpuSummary(platform: PlatformInfo): string {
    const first = platform.cpus[0];
    if (!first) return "unknown";
    const speed = first.speedMHz ? ` @ ${first.speedMHz}MHz` : "";
    return `${platform.cpus.length} × ${first.model}${speed}`;
}

/** A `[os, arch, cpu, memory]` cell tuple for a platform. */
function platformCells(platform: PlatformInfo): string[] {
    const os = `${platform.os.platform} ${platform.os.release}`.trim();
    return [
        os || "unknown",
        platform.os.arch || "—",
        cpuSummary(platform),
        platform.memory.totalBytes
            ? formatBytes(platform.memory.totalBytes)
            : "—",
    ];
}

/** Tool info table (versions, daemon, provisioning, description). */
export function toolsInfoGroup(toolInfo: ToolInfo[]): InfoGroup | null {
    if (toolInfo.length === 0) return null;
    return {
        title: "Tools",
        columns: ["tool", "version", "daemon", "provisioning", "description"],
        rows: toolInfo.map((t) => [
            t.tool,
            t.version ?? "—",
            t.daemon ? "yes" : "no",
            t.provisioning,
            t.description,
        ]),
    };
}

/** Platform table with one row per (deduplicated) target. */
export function platformByTargetGroup(runs: NormalizedRun[]): InfoGroup | null {
    const seen = new Set<string>();
    const rows: string[][] = [];
    for (const r of runs) {
        if (seen.has(r.target)) continue;
        seen.add(r.target);
        rows.push([r.target, ...platformCells(r.platform)]);
    }
    if (rows.length === 0) return null;
    return {
        title: "Platform",
        columns: ["target", "os", "arch", "cpu", "memory"],
        rows,
    };
}

/** Platform table with one row per kept version (in the given order). */
export function platformByVersionGroup(
    kept: Map<string, NormalizedRun>,
    versions: string[],
): InfoGroup | null {
    const rows: string[][] = [];
    for (const v of versions) {
        const r = kept.get(v);
        if (r) rows.push([v, ...platformCells(r.platform)]);
    }
    if (rows.length === 0) return null;
    return {
        title: "Platform",
        columns: ["version", "os", "arch", "cpu", "memory"],
        rows,
    };
}
