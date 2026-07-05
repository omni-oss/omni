#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
    Command,
    Option,
    type OptionValues,
} from "@commander-js/extra-typings";
import { description, name, version } from "../../package.json";
import {
    type BenchEvent,
    type BenchmarkResult,
    DEPENDENCY_STRATEGIES,
    formatMs,
    formatReport,
    formatSuiteMarkdown,
    generateWorkspace,
    getPreset,
    type HarnessConfigInput,
    installWorkspace,
    listPresets,
    parseSuite,
    resolveConfig,
    runBenchmark,
    runSuite,
    type SuiteEvent,
    type SuiteResult,
    TOOLS,
    type Tool,
} from "..";

const program = new Command();
program.name(name).version(version).description(description);

const int = (raw: string): number => {
    const n = Number.parseInt(raw, 10);
    if (Number.isNaN(n)) throw new Error(`expected an integer, got "${raw}"`);
    return n;
};
const float = (raw: string): number => {
    const n = Number.parseFloat(raw);
    if (Number.isNaN(n)) throw new Error(`expected a number, got "${raw}"`);
    return n;
};
const toolList = (raw: string): Tool[] =>
    raw.split(",").map((s) => {
        const t = s.trim();
        if (!(TOOLS as readonly string[]).includes(t)) {
            throw new Error(
                `unknown tool "${t}" (allowed: ${TOOLS.join(", ")})`,
            );
        }
        return t as Tool;
    });

interface GenerateOpts {
    projects?: number;
    tasks?: number;
    strategy?: string;
    layers?: number;
    fanout?: number;
    edgeProbability?: number;
    logLines?: number;
    work?: number;
    outputFiles?: number;
    seed?: number;
    tools?: Tool[];
    chain?: boolean;
    fanUpstream?: boolean;
    turboVersion?: string;
    nxVersion?: string;
    moonVersion?: string;
    bunVersion?: string;
    config?: string;
}

/** Merge a base config file (if any) with CLI overrides into a config input. */
function buildInput(opts: GenerateOpts): HarnessConfigInput {
    const base: HarnessConfigInput = opts.config
        ? JSON.parse(readFileSync(opts.config, "utf8"))
        : {};

    const input: HarnessConfigInput = { ...base };
    if (opts.seed !== undefined) input.seed = opts.seed;
    if (opts.projects !== undefined) input.projects = opts.projects;
    if (opts.tasks !== undefined) input.tasksPerProject = opts.tasks;
    if (opts.tools !== undefined) input.tools = opts.tools;

    const dependency: NonNullable<HarnessConfigInput["dependency"]> = {
        ...(base.dependency ?? {}),
    };
    if (opts.strategy !== undefined)
        dependency.strategy = opts.strategy as NonNullable<
            typeof dependency.strategy
        >;
    if (opts.layers !== undefined) dependency.layers = opts.layers;
    if (opts.fanout !== undefined) dependency.fanout = opts.fanout;
    if (opts.edgeProbability !== undefined)
        dependency.edgeProbability = opts.edgeProbability;
    if (Object.keys(dependency).length) input.dependency = dependency;

    const task: NonNullable<HarnessConfigInput["task"]> = {
        ...(base.task ?? {}),
    };
    if (opts.logLines !== undefined) task.logLines = opts.logLines;
    if (opts.work !== undefined) task.workIterations = opts.work;
    if (opts.outputFiles !== undefined) task.outputFiles = opts.outputFiles;
    if (opts.chain !== undefined) task.chainWithinProject = opts.chain;
    if (opts.fanUpstream !== undefined) task.fanUpstream = opts.fanUpstream;
    if (Object.keys(task).length) input.task = task;

    const versions: NonNullable<HarnessConfigInput["versions"]> = {
        ...(base.versions ?? {}),
    };
    if (opts.turboVersion !== undefined) versions.turbo = opts.turboVersion;
    if (opts.nxVersion !== undefined) versions.nx = opts.nxVersion;
    if (opts.moonVersion !== undefined) versions.moon = opts.moonVersion;
    if (opts.bunVersion !== undefined) versions.bun = opts.bunVersion;
    if (Object.keys(versions).length) input.versions = versions;

    return input;
}

function addGenerateOptions<
    Args extends unknown[],
    Opts extends OptionValues,
    Global extends OptionValues,
>(cmd: Command<Args, Opts, Global>) {
    return cmd
        .option("--config <file>", "Base config JSON file to extend.")
        .option("--seed <n>", "Deterministic graph seed.", int)
        .option("--projects <n>", "Number of projects.", int)
        .option("--tasks <n>", "Tasks per project.", int)
        .addOption(
            new Option(
                "--strategy <name>",
                "Dependency graph strategy.",
            ).choices(DEPENDENCY_STRATEGIES),
        )
        .option("--layers <n>", "Layers for the `layered` strategy.", int)
        .option("--fanout <n>", "Max upstream deps per project.", int)
        .option(
            "--edge-probability <p>",
            "Edge probability for `random`.",
            float,
        )
        .option("--log-lines <n>", "Log lines printed per task.", int)
        .option("--work <n>", "CPU work iterations per task.", int)
        .option("--output-files <n>", "Output files per task.", int)
        .option(
            "--tools <list>",
            "Comma-separated tools (omni,turbo,nx,moon).",
            toolList,
        )
        .option("--turbo-version <semver>", "Turbo version to install.")
        .option("--nx-version <semver>", "Nx version to install.")
        .option(
            "--moon-version <semver>",
            "moon (@moonrepo/cli) version to install.",
        )
        .option("--bun-version <semver>", "bun version for packageManager.")
        .option("--no-chain", "Disable intra-project task chaining.")
        .option("--no-fan-upstream", "Disable upstream (^) task dependencies.");
}

function progressHandler(): (event: BenchEvent) => void {
    return (event) => {
        if (event.kind === "tool-start") {
            process.stderr.write(`\n▶ ${event.tool}\n`);
        } else if (event.kind === "tool-error") {
            process.stderr.write(`  ✖ ${event.tool}: ${event.error}\n`);
        } else {
            const status = event.sample.ok
                ? "ok"
                : `exit ${event.sample.exitCode}`;
            process.stderr.write(
                `  ${event.scenario} ${event.run}/${event.total}: ` +
                    `${formatMs(event.sample.durationMs)} ` +
                    `(ran ${event.sample.executed} tasks, ${status})\n`,
            );
        }
    };
}

function writeJson(file: string | undefined, result: BenchmarkResult): void {
    if (!file) return;
    writeFileSync(file, `${JSON.stringify(result, null, 2)}\n`);
    process.stderr.write(`\nWrote results to ${file}\n`);
}

// --- generate ---------------------------------------------------------------

addGenerateOptions(
    program
        .command("generate")
        .alias("gen")
        .description("Generate a benchmark workspace at the given root dir.")
        .requiredOption(
            "-o, --out <dir>",
            "Root dir to generate the workspace into.",
        ),
).action(async (opts) => {
    const out = resolve(opts.out);
    const input = buildInput(opts);
    const result = await generateWorkspace(out, input);
    process.stderr.write(
        `Generated ${result.projects.length} projects ` +
            `(${result.files.length} files) at ${out}\n` +
            `Tools: ${result.config.tools.join(", ")}\n`,
    );
});

// --- run ---------------------------------------------------------------------

program
    .command("run")
    .description("Benchmark an already-generated workspace.")
    .requiredOption("-d, --dir <dir>", "Root dir of a generated workspace.")
    .option("--tools <list>", "Comma-separated tools to benchmark.", toolList)
    .option("--task <name>", "Task to run (defaults to the last task).")
    .option("--cold-runs <n>", "Cold (uncached) runs per tool.", int, 3)
    .option("--warm-runs <n>", "Warm (cached) runs per tool.", int, 5)
    .option(
        "--concurrency <n>",
        "Max parallel tasks, applied identically to all tools (default: CPU count).",
        int,
    )
    .option("--no-daemon", "Disable each tool's persistent daemon (turbo, nx).")
    .option("--json <file>", "Write full results as JSON to this file.")
    .action(async (opts) => {
        const dir = resolve(opts.dir);
        const result = await runBenchmark(dir, {
            tools: opts.tools,
            task: opts.task,
            concurrency: opts.concurrency,
            daemon: opts.daemon,
            coldRuns: opts.coldRuns,
            warmRuns: opts.warmRuns,
            onEvent: progressHandler(),
        });
        process.stdout.write(formatReport(result));
        writeJson(opts.json, result);
    });

// --- bench (generate + install + run) ---------------------------------------

addGenerateOptions(
    program
        .command("bench")
        .description("Generate, install, and benchmark in one step.")
        .requiredOption(
            "-o, --out <dir>",
            "Root dir to generate the workspace into.",
        )
        .option(
            "--no-install",
            "Skip `bun install` in the generated workspace.",
        )
        .option("--task <name>", "Task to run (defaults to the last task).")
        .option("--cold-runs <n>", "Cold (uncached) runs per tool.", int, 3)
        .option("--warm-runs <n>", "Warm (cached) runs per tool.", int, 5)
        .option(
            "--concurrency <n>",
            "Max parallel tasks, applied identically to all tools (default: CPU count).",
            int,
        )
        .option(
            "--no-daemon",
            "Disable each tool's persistent daemon (turbo, nx).",
        )
        .option("--json <file>", "Write full results as JSON to this file."),
).action(async (opts) => {
    const out = resolve(opts.out);
    const input = buildInput(opts);
    const generated = await generateWorkspace(out, input);
    process.stderr.write(
        `Generated ${generated.projects.length} projects at ${out}\n`,
    );

    if (opts.install !== false) {
        process.stderr.write("Installing dependencies (bun install)...\n");
        await installWorkspace(out);
    }

    const result = await runBenchmark(out, {
        task: opts.task,
        coldRuns: opts.coldRuns,
        warmRuns: opts.warmRuns,
        concurrency: opts.concurrency,
        daemon: opts.daemon,
        onEvent: progressHandler(),
    });
    process.stdout.write(formatReport(result));
    writeJson(opts.json, result);
});

// --- inspect (no side effects) ----------------------------------------------

addGenerateOptions(
    program
        .command("inspect")
        .description("Resolve a config and print the derived graph summary."),
).action((opts) => {
    const config = resolveConfig(buildInput(opts));
    process.stdout.write(`${JSON.stringify(config, null, 2)}\n`);
});

// --- suite (run a preset of scenarios) --------------------------------------

function suiteProgressHandler(): (event: SuiteEvent) => void {
    const bench = progressHandler();
    return (event) => {
        if (event.kind === "scenario-start") {
            process.stderr.write(
                `\n=== [${event.index + 1}/${event.total}] ${event.name} ===\n`,
            );
        } else if (event.kind === "bench") {
            bench(event.event);
        }
    };
}

program
    .command("suite")
    .description(
        "Run a preset (or JSON file) of benchmark scenarios and summarize them.",
    )
    .option("-p, --preset <name>", "Built-in preset name.")
    .option("-f, --file <path>", "Path to a JSON suite preset file.")
    .option("-o, --out <dir>", "Working dir for generated workspaces.")
    .option("--json <file>", "Write aggregated results as JSON to this file.")
    .option(
        "--md <file>",
        "Write a human-readable Markdown report to this file.",
    )
    .option("--tools <list>", "Override tools for every scenario.", toolList)
    .option("--cold-runs <n>", "Override cold runs for every scenario.", int)
    .option("--warm-runs <n>", "Override warm runs for every scenario.", int)
    .option(
        "--concurrency <n>",
        "Override concurrency for every scenario.",
        int,
    )
    .option("--no-install", "Skip `bun install` in generated workspaces.")
    .option("--keep", "Keep generated workspaces instead of removing them.")
    .option("--list", "List the built-in presets and exit.")
    .action(async (opts) => {
        if (opts.list) {
            process.stdout.write(
                `Available presets:\n${listPresets()
                    .map((p) => `  - ${p}`)
                    .join("\n")}\n`,
            );
            return;
        }

        const suite = opts.file
            ? parseSuite(JSON.parse(readFileSync(opts.file, "utf8")))
            : getPreset(opts.preset ?? "quick");

        const workdir = resolve(opts.out ?? join(tmpdir(), "task-bench-suite"));
        process.stderr.write(
            `Running suite "${suite.name}" (${suite.scenarios.length} scenarios) in ${workdir}\n`,
        );

        const overrides = {
            ...(opts.tools ? { tools: opts.tools } : {}),
            ...(opts.coldRuns !== undefined ? { coldRuns: opts.coldRuns } : {}),
            ...(opts.warmRuns !== undefined ? { warmRuns: opts.warmRuns } : {}),
            ...(opts.concurrency !== undefined
                ? { concurrency: opts.concurrency }
                : {}),
        };

        const result: SuiteResult = await runSuite(suite, {
            workdir,
            install: opts.install,
            keep: opts.keep,
            overrides,
            onEvent: suiteProgressHandler(),
        });

        const md = formatSuiteMarkdown(result);
        process.stdout.write(`\n${md}\n`);
        if (opts.md) {
            writeFileSync(opts.md, `${md}\n`);
            process.stderr.write(`\nWrote Markdown report to ${opts.md}\n`);
        }
        if (opts.json) {
            writeFileSync(opts.json, `${JSON.stringify(result, null, 2)}\n`);
            process.stderr.write(`Wrote JSON results to ${opts.json}\n`);
        }
    });

program.parseAsync();
