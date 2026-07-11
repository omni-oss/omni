import type { Tool } from "@omni-oss/task-bench";
import type { TargetId } from "../sources/types";

/**
 * The dashboard's internal normalized model. Analyzers consume only this; they
 * never import task-bench types. See DESIGN.md §5.2.
 */

export type { Tool };
export type Warmth = "cold" | "warm";
export type Metric =
    | "durationMs"
    | "peakRssBytes"
    | "cpuTimeMs"
    | "parallelism";

/** A CPU as reported by the benchmark host. */
export interface CpuInfo {
    model: string;
    speedMHz: number;
}

/** The machine a benchmark artifact ran on. */
export interface PlatformInfo {
    cpus: CpuInfo[];
    memory: { totalBytes: number; freeBytes: number };
    os: { platform: string; release: string; arch: string };
}

/** Noteworthy attributes of a benchmarked tool. */
export interface ToolInfo {
    tool: Tool;
    version: string | null;
    daemon: boolean;
    provisioning: string;
    supportedVersions: string[];
    description: string;
}

/** Fully-qualified coordinate of a single measurement. */
export interface Coord {
    /** omni version this artifact belongs to. */
    version: string;
    target: TargetId;
    /** `platform.os.platform` — "linux" | "win32" | "darwin". */
    os: string;
    /** Preset name (`SuiteResult.name`). */
    preset: string;
    /** Scenario name (`scenario.name`, e.g. "scale-300"). */
    scenario: string;
    tool: Tool;
    toolVersion: string | null;
    warmth: Warmth;
}

/** One metric summary at a coordinate (mirrors the harness `Stats`). */
export interface SamplePoint {
    coord: Coord;
    metric: Metric;
    median: number;
    mean: number;
    min: number;
    max: number;
    stddev: number;
    /** Sample count backing the stat (0 ⇒ empty). */
    n: number;
    /** Whether resource metrics existed for this scenario. */
    hasResources: boolean;
    /** Whether `tool.error` was set for this scenario. */
    errored: boolean;
}

/** One artifact, normalized: provenance + the flattened points it produced. */
export interface NormalizedRun {
    /** DataSource id. */
    source: string;
    version: string;
    target: TargetId;
    os: string;
    preset: string;
    generatedAt: string;
    taskBenchVersion: string;
    concurrency: number;
    daemon: boolean;
    /** The machine this artifact ran on. */
    platform: PlatformInfo;
    /** Noteworthy attributes of each benchmarked tool. */
    toolInfo: ToolInfo[];
    /** Carried from the RunRef: enables "view source at this build" links. */
    commitSha?: string;
    sourceUrl?: string;
    points: SamplePoint[];
    /** Non-fatal ingest notes (dropped tool, missing resources, empty stats…). */
    warnings: string[];
}
