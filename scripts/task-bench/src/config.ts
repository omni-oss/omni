import { z } from "zod";

/**
 * The supported inter-project dependency graph shapes. These control how much
 * of a task graph a runner has to walk during discovery / scheduling, which is
 * what we are trying to measure.
 */
export const DEPENDENCY_STRATEGIES = [
    "isolated",
    "chain",
    "fan-out",
    "layered",
    "random",
] as const;

export const DependencyStrategySchema = z.enum(DEPENDENCY_STRATEGIES);
export type DependencyStrategy = z.infer<typeof DependencyStrategySchema>;

/** The task runners we know how to generate configuration for and benchmark. */
export const TOOLS = ["omni", "turbo", "nx", "moon"] as const;
export const ToolSchema = z.enum(TOOLS);
export type Tool = z.infer<typeof ToolSchema>;

export const DependencyConfigSchema = z
    .object({
        strategy: DependencyStrategySchema.default("layered").describe(
            "Shape of the inter-project dependency graph.",
        ),
        layers: z
            .number()
            .int()
            .positive()
            .default(5)
            .describe("Number of layers for the `layered` strategy."),
        fanout: z
            .number()
            .int()
            .nonnegative()
            .default(3)
            .describe(
                "Maximum number of upstream dependencies per project (cap for `layered`/`random`).",
            ),
        edgeProbability: z
            .number()
            .min(0)
            .max(1)
            .default(0.35)
            .describe("Edge inclusion probability for the `random` strategy."),
    })
    .prefault({});

export const TaskConfigSchema = z
    .object({
        logLines: z
            .number()
            .int()
            .nonnegative()
            .default(25)
            .describe("How many log lines each task prints to stdout."),
        workIterations: z
            .number()
            .int()
            .nonnegative()
            .default(150_000)
            .describe(
                "Iterations of cheap CPU work per task. Keep small so caching dominates.",
            ),
        outputFiles: z
            .number()
            .int()
            .positive()
            .default(1)
            .describe("Number of output files each task writes into dist/."),
        chainWithinProject: z
            .boolean()
            .default(true)
            .describe(
                "Whether task `tN` depends on `t(N-1)` within a project.",
            ),
        fanUpstream: z
            .boolean()
            .default(true)
            .describe(
                "Whether task `tN` depends on `tN` of upstream projects (^tN).",
            ),
    })
    .prefault({});

export const VersionsConfigSchema = z
    .object({
        turbo: z.string().default("2.10.3"),
        nx: z.string().default("23.0.1"),
        moon: z.string().default("2.3.5"),
        bun: z.string().default("1.3.14"),
    })
    .prefault({});

export const HarnessConfigSchema = z.object({
    seed: z
        .number()
        .int()
        .nonnegative()
        .default(1)
        .describe("Seed for deterministic graph generation."),
    projectPrefix: z
        .string()
        .regex(/^[a-z][a-z0-9-]*$/)
        .default("bench-p")
        .describe("Prefix used for generated package names."),
    projects: z
        .number()
        .int()
        .positive()
        .default(50)
        .describe("Number of projects to generate."),
    tasksPerProject: z
        .number()
        .int()
        .positive()
        .default(3)
        .describe("Number of tasks (t0..tN-1) per project."),
    dependency: DependencyConfigSchema,
    task: TaskConfigSchema,
    tools: z
        .array(ToolSchema)
        .min(1)
        .default([...TOOLS])
        .describe("Which runners to configure and benchmark."),
    versions: VersionsConfigSchema,
});

export type HarnessConfig = z.infer<typeof HarnessConfigSchema>;
export type HarnessConfigInput = z.input<typeof HarnessConfigSchema>;

/** Parse and fill defaults for a (possibly partial) harness config. */
export function resolveConfig(input?: HarnessConfigInput): HarnessConfig {
    return HarnessConfigSchema.parse(input ?? {});
}
