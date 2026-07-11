import type { SuiteResult } from "@omni-oss/task-bench";
import { z } from "zod";

/**
 * Runtime validation for the task-bench `data.json` payload. task-bench is the
 * producer, so its exported types are the authoritative shape; this schema is a
 * *lenient subset* covering only the fields the dashboard reads, tied to the
 * task-bench type by a compile-time drift guard below. See DESIGN.md §5.1.
 *
 * This is one of only two modules coupled to task-bench output (the other is
 * `normalize.ts`). Everything downstream sees only the normalized model.
 */

const StatsSchema = z.object({
    samples: z.array(z.number()),
    min: z.number(),
    max: z.number(),
    mean: z.number(),
    median: z.number(),
    stddev: z.number(),
});

const ResourceStatsSchema = z.object({
    runs: z.number(),
    peakRssBytes: StatsSchema,
    cpuTimeMs: StatsSchema,
    parallelism: StatsSchema,
});

const ScenarioResultSchema = z.object({
    runs: z.number(),
    failures: z.number(),
    stats: StatsSchema,
    executedMedian: z.number(),
    resources: ResourceStatsSchema.optional(),
});

const ToolResultSchema = z.object({
    tool: z.string(),
    task: z.string(),
    taskGraphSize: z.number(),
    cold: ScenarioResultSchema,
    warm: ScenarioResultSchema,
    error: z.string().optional(),
});

const BenchmarkResultSchema = z.object({
    concurrency: z.number(),
    daemon: z.boolean(),
    versions: z.record(z.string(), z.string().nullable()),
    tools: z.array(ToolResultSchema),
    toolInfo: z
        .array(
            z.object({
                tool: z.string(),
                version: z.string().nullable(),
                daemon: z.boolean(),
                provisioning: z.string(),
                supportedVersions: z.array(z.string()),
                description: z.string(),
            }),
        )
        .optional(),
    platform: z.object({
        cpus: z
            .array(z.object({ model: z.string(), speedMHz: z.number() }))
            .optional(),
        memory: z
            .object({ totalBytes: z.number(), freeBytes: z.number() })
            .optional(),
        os: z.object({
            platform: z.string(),
            release: z.string(),
            arch: z.string(),
        }),
    }),
});

const ScenarioSchema = z.object({
    name: z.string(),
    result: BenchmarkResultSchema,
});

export const SuiteResultSchema = z.object({
    name: z.string(),
    generatedAt: z.string(),
    taskBenchVersion: z.string(),
    scenarios: z.array(ScenarioSchema),
});

/** The validated subset the dashboard consumes. */
export type ParsedSuite = z.infer<typeof SuiteResultSchema>;

// ---------------------------------------------------------------------------
// Compile-time drift guard.
//
// A real `SuiteResult` from task-bench must remain assignable to our parsed
// subset. If task-bench renames, removes, or retypes any field we depend on,
// this stops compiling — pointing straight at the fields to update here and in
// `normalize.ts`.
// ---------------------------------------------------------------------------
type Assert<T extends true> = T;
type _DriftGuard = Assert<SuiteResult extends ParsedSuite ? true : never>;

/** Parse + validate a raw `data.json` string into the consumed subset. */
export function parseSuite(json: string): ParsedSuite {
    return SuiteResultSchema.parse(JSON.parse(json));
}
