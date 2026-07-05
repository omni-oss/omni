import { rm } from "node:fs/promises";
import { join } from "node:path";
import { version } from "../../package.json";
import { type BenchEvent, type BenchmarkResult, runBenchmark } from "../bench";
import { installWorkspace } from "../bench/install";
import type { HarnessConfig } from "../config";
import { generateWorkspace } from "../generate";
import { type RunOptions, resolveScenario, type SuiteConfig } from "./preset";

export interface SuiteScenarioResult {
    name: string;
    description?: string | undefined;
    config: HarnessConfig;
    run: RunOptions;
    result: BenchmarkResult;
}

export interface SuiteResult {
    name: string;
    description?: string | undefined;
    generatedAt: string;
    scenarios: SuiteScenarioResult[];
    taskBenchVersion: string;
}

export type SuiteEvent =
    | { kind: "scenario-start"; name: string; index: number; total: number }
    | { kind: "scenario-done"; name: string; index: number; total: number }
    | { kind: "bench"; name: string; event: BenchEvent };

export interface RunSuiteOptions {
    /** Base directory under which each scenario workspace is generated. */
    workdir: string;
    /** Run `bun install` in each generated workspace (default true). */
    install?: boolean | undefined;
    /** Keep generated workspaces on disk instead of removing them (default false). */
    keep?: boolean | undefined;
    /** Global run-option overrides applied to every scenario. */
    overrides?: Partial<RunOptions> | undefined;
    onEvent?: ((event: SuiteEvent) => void) | undefined;
}

/** Drop `undefined` values so an override map only carries explicit settings. */
function compact<T extends object>(obj: T): Partial<T> {
    return Object.fromEntries(
        Object.entries(obj).filter(([, v]) => v !== undefined),
    ) as Partial<T>;
}

/**
 * Run every scenario in a suite: generate a workspace, install, benchmark, and
 * collect the results. Workspaces are removed afterwards unless `keep` is set.
 */
export async function runSuite(
    suite: SuiteConfig,
    options: RunSuiteOptions,
): Promise<SuiteResult> {
    const emit = options.onEvent ?? (() => {});
    const scenarios: SuiteScenarioResult[] = [];
    const total = suite.scenarios.length;

    for (const [index, scenario] of suite.scenarios.entries()) {
        const resolved = resolveScenario(suite, scenario);
        const { config, description, name } = resolved;

        // Global overrides win over per-scenario/defaults.
        const run: RunOptions = {
            ...resolved.run,
            ...compact(options.overrides ?? {}),
        };
        // Keep generation and execution tool sets consistent.
        const tools = run.tools ?? config.tools;
        config.tools = tools;

        const dir = join(options.workdir, name);

        emit({ kind: "scenario-start", name, index, total });

        await generateWorkspace(dir, config);
        if (options.install !== false) {
            await installWorkspace(dir, { quiet: true });
        }

        const result = await runBenchmark(dir, {
            ...run,
            tools,
            onEvent: (event) => emit({ kind: "bench", name, event }),
        });

        scenarios.push({ name, description, config, run, result });

        if (!options.keep) {
            await rm(dir, { recursive: true, force: true });
        }

        emit({ kind: "scenario-done", name, index, total });
    }

    return {
        name: suite.name,
        description: suite.description,
        generatedAt: new Date().toISOString(),
        scenarios,
        taskBenchVersion: version,
    };
}
