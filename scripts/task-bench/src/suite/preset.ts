import { z } from "zod";
import {
    type HarnessConfig,
    type HarnessConfigInput,
    resolveConfig,
    ToolSchema,
} from "../config";

/** Per-scenario benchmark run options (serializable subset of RunBenchmarkOptions). */
export const RunOptionsSchema = z
    .object({
        tools: z.array(ToolSchema).optional(),
        task: z.string().optional(),
        coldRuns: z.number().int().nonnegative().optional(),
        warmRuns: z.number().int().nonnegative().optional(),
        concurrency: z.number().int().positive().optional(),
        daemon: z.boolean().optional(),
    })
    .default({});

export type RunOptions = z.infer<typeof RunOptionsSchema>;

/**
 * A harness config fragment. Kept loose here (merged then validated by
 * `resolveConfig`) so scenarios can inherit from suite-level defaults.
 */
const ConfigFragmentSchema = z.record(z.string(), z.unknown()).default({});

export const ScenarioSchema = z.object({
    name: z
        .string()
        .regex(
            /^[a-zA-Z0-9._-]+$/,
            "scenario name must be filesystem-safe (letters, digits, . _ -)",
        ),
    description: z.string().optional(),
    config: ConfigFragmentSchema,
    run: RunOptionsSchema,
});

export const SuiteSchema = z.object({
    name: z.string().default("task-bench suite"),
    description: z.string().optional(),
    defaults: z
        .object({
            config: ConfigFragmentSchema,
            run: RunOptionsSchema,
        })
        .prefault({}),
    scenarios: z.array(ScenarioSchema).min(1),
});

export type SuiteConfig = z.infer<typeof SuiteSchema>;
export type SuiteConfigInput = z.input<typeof SuiteSchema>;
export type Scenario = z.infer<typeof ScenarioSchema>;

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Deep-merge plain objects; arrays and scalars from `override` replace `base`. */
export function deepMerge(
    base: Record<string, unknown>,
    override: Record<string, unknown>,
): Record<string, unknown> {
    const out: Record<string, unknown> = { ...base };
    for (const [key, value] of Object.entries(override)) {
        const current = out[key];
        out[key] =
            isPlainObject(current) && isPlainObject(value)
                ? deepMerge(current, value)
                : value;
    }
    return out;
}

export interface ResolvedScenario {
    name: string;
    description?: string | undefined;
    config: HarnessConfig;
    run: RunOptions;
}

/** Merge a scenario over suite defaults and resolve/validate its config. */
export function resolveScenario(
    suite: SuiteConfig,
    scenario: Scenario,
): ResolvedScenario {
    const mergedConfig = deepMerge(
        suite.defaults.config,
        scenario.config,
    ) as HarnessConfigInput;
    const run: RunOptions = { ...suite.defaults.run, ...scenario.run };
    return {
        name: scenario.name,
        description: scenario.description,
        config: resolveConfig(mergedConfig),
        run,
    };
}

// ---------------------------------------------------------------------------
// Built-in presets
// ---------------------------------------------------------------------------

const DEPENDENCY_STRATEGIES_FOR_SWEEP = [
    "isolated",
    "chain",
    "fan-out",
    "layered",
    "random",
] as const;

const shapes: SuiteConfigInput = {
    name: "dependency-shape sweep",
    description:
        "How the dependency-graph shape affects discovery/scheduling overhead at a fixed scale.",
    defaults: {
        config: { projects: 120, tasksPerProject: 3 },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: DEPENDENCY_STRATEGIES_FOR_SWEEP.map((strategy) => ({
        name: `shape-${strategy}`,
        config: { dependency: { strategy } },
    })),
};

const scale: SuiteConfigInput = {
    name: "scale sweep",
    description: "How overhead grows with workspace size (layered graph).",
    defaults: {
        config: {
            tasksPerProject: 3,
            dependency: { strategy: "layered", layers: 8 },
        },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: [50, 150, 300, 600].map((projects) => ({
        name: `scale-${projects}`,
        config: { projects },
    })),
};

const density: SuiteConfigInput = {
    name: "task-density sweep",
    description: "How the number of tasks per project affects overhead.",
    defaults: {
        config: { projects: 120, dependency: { strategy: "layered" } },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: [2, 5, 10].map((tasksPerProject) => ({
        name: `density-${tasksPerProject}`,
        config: { tasksPerProject },
    })),
};

const daemon: SuiteConfigInput = {
    name: "daemon on vs off",
    description:
        "Whether each tool's persistent daemon changes warm/cold overhead.",
    defaults: {
        config: {
            projects: 200,
            tasksPerProject: 3,
            dependency: { strategy: "layered" },
        },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: [
        { name: "daemon-on", run: { daemon: true } },
        { name: "daemon-off", run: { daemon: false } },
    ],
};

const quick: SuiteConfigInput = {
    name: "quick smoke suite",
    description: "Tiny, fast sanity sweep across two shapes.",
    defaults: {
        config: { projects: 30, tasksPerProject: 2 },
        run: { concurrency: 8, coldRuns: 1, warmRuns: 2 },
    },
    scenarios: [
        {
            name: "quick-isolated",
            config: { dependency: { strategy: "isolated" } },
        },
        {
            name: "quick-layered",
            config: { dependency: { strategy: "layered" } },
        },
    ],
};

const full: SuiteConfigInput = {
    name: "full suite",
    description: "Shapes + scale + density in one run.",
    defaults: { run: { concurrency: 8, coldRuns: 2, warmRuns: 3 } },
    scenarios: [
        ...DEPENDENCY_STRATEGIES_FOR_SWEEP.map((strategy) => ({
            name: `shape-${strategy}`,
            config: {
                projects: 120,
                tasksPerProject: 3,
                dependency: { strategy },
            },
        })),
        ...[50, 150, 300].map((projects) => ({
            name: `scale-${projects}`,
            config: {
                projects,
                tasksPerProject: 3,
                dependency: { strategy: "layered" as const, layers: 8 },
            },
        })),
        ...[2, 5, 10].map((tasksPerProject) => ({
            name: `density-${tasksPerProject}`,
            config: {
                projects: 120,
                tasksPerProject,
                dependency: { strategy: "layered" as const },
            },
        })),
    ],
};

export const BUILTIN_PRESETS: Record<string, SuiteConfigInput> = {
    quick,
    shapes,
    scale,
    density,
    daemon,
    full,
};

export function listPresets(): string[] {
    return Object.keys(BUILTIN_PRESETS);
}

/** Resolve a built-in preset by name, or throw with the list of valid names. */
export function getPreset(name: string): SuiteConfig {
    const preset = BUILTIN_PRESETS[name];
    if (!preset) {
        throw new Error(
            `unknown preset "${name}" (available: ${listPresets().join(", ")})`,
        );
    }
    return SuiteSchema.parse(preset);
}

/** Parse and validate a suite preset object (e.g. loaded from a JSON file). */
export function parseSuite(input: unknown): SuiteConfig {
    return SuiteSchema.parse(input);
}
