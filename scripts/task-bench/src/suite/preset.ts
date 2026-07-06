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
        resourceRuns: z.number().int().nonnegative().optional(),
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
    /** Human-friendly label for reports; falls back to `name` when omitted. */
    displayName: z.string().optional(),
    description: z.string().optional(),
    config: ConfigFragmentSchema,
    run: RunOptionsSchema,
});

export const SuiteSchema = z.object({
    name: z.string().default("task-bench suite"),
    /** Human-friendly label for reports; falls back to `name` when omitted. */
    displayName: z.string().optional(),
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
    /** Resolved display label (`displayName ?? name`). */
    displayName: string;
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
        displayName: scenario.displayName ?? scenario.name,
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

type SweepStrategy = (typeof DEPENDENCY_STRATEGIES_FOR_SWEEP)[number];

/** Short, human-friendly label for each dependency-graph shape. */
const STRATEGY_LABELS: Record<SweepStrategy, string> = {
    isolated: "Isolated",
    chain: "Chain",
    "fan-out": "Fan-out",
    layered: "Layered",
    random: "Random",
};

/** What each dependency-graph shape stresses in a task runner. */
const STRATEGY_DESCRIPTIONS: Record<SweepStrategy, string> = {
    isolated:
        "No inter-project dependencies — every task is independent, so the runner can schedule everything in parallel (baseline, minimal graph traversal).",
    chain: "Each project depends on the previous one — a deep, serial dependency chain that minimizes parallelism and maximizes scheduling depth.",
    "fan-out":
        "Every project depends on a single shared root — a wide star graph that stresses fan-out from one common dependency.",
    layered:
        "Projects grouped into dependency layers — a balanced, realistic monorepo graph mixing breadth and depth.",
    random: "Randomized (seeded) dependency edges — an irregular, uneven graph that mimics organically-grown repos.",
};

/** Describe a scale-sweep data point (project count → workspace size band). */
function scaleDescription(projects: number): string {
    const band =
        projects <= 50
            ? "small"
            : projects <= 150
              ? "medium"
              : projects <= 300
                ? "large"
                : "very large";
    return `${band} workspace (${projects} projects) — shows how discovery + caching overhead scales with graph size.`;
}

/** Describe a task-density data point (tasks per project). */
function densityDescription(tasksPerProject: number): string {
    const band =
        tasksPerProject <= 2
            ? "sparse"
            : tasksPerProject <= 5
              ? "moderate"
              : "dense";
    return `${band} per-project graphs (${tasksPerProject} tasks each) — isolates the cost of more tasks per project at a fixed project count.`;
}

const shapes: SuiteConfigInput = {
    name: "dependency-shape sweep",
    displayName: "Dependency-shape sweep",
    description:
        "How the dependency-graph shape affects discovery/scheduling overhead at a fixed scale.",
    defaults: {
        config: { projects: 120, tasksPerProject: 3 },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: DEPENDENCY_STRATEGIES_FOR_SWEEP.map((strategy) => ({
        name: `shape-${strategy}`,
        displayName: STRATEGY_LABELS[strategy],
        description: STRATEGY_DESCRIPTIONS[strategy],
        config: { dependency: { strategy } },
    })),
};

const scale: SuiteConfigInput = {
    name: "scale sweep",
    displayName: "Scale sweep",
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
        displayName: `${projects} projects`,
        description: scaleDescription(projects),
        config: { projects },
    })),
};

const density: SuiteConfigInput = {
    name: "task-density sweep",
    displayName: "Task-density sweep",
    description: "How the number of tasks per project affects overhead.",
    defaults: {
        config: { projects: 120, dependency: { strategy: "layered" } },
        run: { concurrency: 8, coldRuns: 2, warmRuns: 3 },
    },
    scenarios: [2, 5, 10].map((tasksPerProject) => ({
        name: `density-${tasksPerProject}`,
        displayName: `${tasksPerProject} tasks/project`,
        description: densityDescription(tasksPerProject),
        config: { tasksPerProject },
    })),
};

const daemon: SuiteConfigInput = {
    name: "daemon on vs off",
    displayName: "Daemon on vs off",
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
        {
            name: "daemon-on",
            displayName: "Daemon on",
            description:
                "Daemons enabled — each tool may use its persistent background process (Turbo/Nx) to speed up warm runs.",
            run: { daemon: true },
        },
        {
            name: "daemon-off",
            displayName: "Daemon off",
            description:
                "Daemons disabled — every invocation is a cold process, exposing raw start-up + discovery overhead.",
            run: { daemon: false },
        },
    ],
};

const quick: SuiteConfigInput = {
    name: "quick smoke suite",
    displayName: "Quick smoke suite",
    description: "Tiny, fast sanity sweep across two shapes.",
    defaults: {
        config: { projects: 30, tasksPerProject: 2 },
        run: { concurrency: 8, coldRuns: 1, warmRuns: 2 },
    },
    scenarios: [
        {
            name: "quick-isolated",
            displayName: STRATEGY_LABELS.isolated,
            description: STRATEGY_DESCRIPTIONS.isolated,
            config: { dependency: { strategy: "isolated" } },
        },
        {
            name: "quick-layered",
            displayName: STRATEGY_LABELS.layered,
            description: STRATEGY_DESCRIPTIONS.layered,
            config: { dependency: { strategy: "layered" } },
        },
    ],
};

const full: SuiteConfigInput = {
    name: "full suite",
    displayName: "Full suite",
    description: "Shapes + scale + density in one run.",
    defaults: { run: { concurrency: 8, coldRuns: 2, warmRuns: 3 } },
    scenarios: [
        ...DEPENDENCY_STRATEGIES_FOR_SWEEP.map((strategy) => ({
            name: `shape-${strategy}`,
            displayName: `Shape: ${STRATEGY_LABELS[strategy]}`,
            description: STRATEGY_DESCRIPTIONS[strategy],
            config: {
                projects: 120,
                tasksPerProject: 3,
                dependency: { strategy },
            },
        })),
        ...[50, 150, 300].map((projects) => ({
            name: `scale-${projects}`,
            displayName: `Scale: ${projects} projects`,
            description: scaleDescription(projects),
            config: {
                projects,
                tasksPerProject: 3,
                dependency: { strategy: "layered" as const, layers: 8 },
            },
        })),
        ...[2, 5, 10].map((tasksPerProject) => ({
            name: `density-${tasksPerProject}`,
            displayName: `Density: ${tasksPerProject} tasks/project`,
            description: densityDescription(tasksPerProject),
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
