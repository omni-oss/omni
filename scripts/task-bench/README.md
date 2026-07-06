# @omni-oss/task-bench

Generate configurable, minimal test workspaces and benchmark **task-execution
overhead** across [omni](https://github.com/omni-oss/omni),
[Turborepo](https://turbo.build), [Nx](https://nx.dev), and
[moonrepo](https://moonrepo.dev).

The goal is to isolate the cost of **project discovery** and **task caching**
rather than the tasks themselves. Every generated task is fast, deterministic,
and produces real output (logs + files) so each runner's cache and log-capture
machinery is exercised. The four runners are configured as equivalently as
possible from a single source of truth.

## How it works

For a given config the harness generates one workspace on disk containing:

- `packages/<prefix><n>/` — minimal JS projects, each with:
  - `package.json` (scripts + `workspace:*` deps encoding the project graph),
  - `src/index.js` (a cache input),
  - `task.mjs` (the shared, deterministic task runner),
  - `project.omni.yaml` (omni), `project.json` (nx), and `moon.yml` (moon).
- Root `workspace.omni.yaml`, `turbo.json`, `nx.json`, and `.moon/*.yml`
  describing the same task graph (`tN` depends on `t(N-1)` and/or `^tN`) with
  identical inputs (`package.json`, `task.mjs`, `src/**`) and outputs
  (`dist/tN.*`). The workspace is also `git init`-ed and committed, which moon
  requires to enable its cache (and is harmless/realistic for the others).

It then benchmarks each enabled tool in two scenarios:

- **cold** — caches + outputs wiped before each run (discovery + full exec).
- **warm** — caches primed, so every task should be a cache hit. This is the
  key metric for discovery + cache-restore overhead.

### Fairness & correctness

- **Verified cache hits.** Each task appends one line to an out-of-tree log
  *only when it actually executes* (cache hits skip the process entirely).
  The harness counts these to report a real, tool-agnostic cache-hit rate per
  run, so "warm == all cached" is verified rather than assumed. A warm run that
  is not 100% cached is flagged in the report. (Turbo's strict env mode is
  handled via `globalPassThroughEnv` so the marker survives without affecting
  cache keys.)
- **Concurrency parity.** The same max-parallelism is passed to every runner
  (`omni -c`, `turbo --concurrency`, `nx --parallel`); defaults to the CPU
  count. Use `--concurrency` to pin it.
- **Daemon handling.** By default each tool is allowed to use its persistent
  daemon (Turbo's `turbod`, the Nx daemon) so warm runs reflect each tool's
  best incremental performance; the daemon is warmed by an unmeasured prime run
  and left alive across warm runs. Cold runs deliberately tear the daemon down
  (and clear caches) so they include cold-start cost. Daemons are always
  stopped as cleanup afterwards so nothing leaks. Pass `--no-daemon` to disable
  daemons entirely (Turbo `--no-daemon`, `NX_DAEMON=false`); omni and moon have
  no daemon.
- **Statistics.** Reports median ± standard deviation; full per-run samples
  (duration, exit code, executed-task count) are written to the JSON output.
- **Resource usage.** Peak RSS and CPU time of the *entire process tree an
  invocation spawns* — the CLI, its task workers, and any persistent daemon —
  are measured in dedicated passes, kept separate from the timed runs so the
  resource probe can never inflate the reported durations. Sampling uses a
  native, cross-platform probe: `/proc` on Linux, `ps` on macOS, and on Windows
  a persistent PowerShell `Get-Process` sampler (fast per-PID RSS/CPU) plus an
  occasional `Get-CimInstance` tree walk for parent/child discovery — no `wmic`,
  so it works on Windows 11 24H2+. The tree is rooted at the CLI process and any
  daemon PID each adapter locates (`turbo daemon status`, `nx daemon`); omni and
  moon have none. CPU is summed from each process's cumulative CPU-time counter
  (immune to short-lived spikes) and reported with average parallelism
  (`cpu-time / wall-time`); a *persistent* daemon only contributes its `ctime`
  delta for the run, while a daemon the run starts itself (cold) counts in full.
  Peak RSS is a *sampled maximum* and both metrics are lower bounds: a process
  that spawns and exits entirely between two samples is missed. Set
  `--resource-runs 0` to skip it.

## CLI

```sh
# Generate a workspace only.
task-bench generate -o /tmp/bench --projects 200 --tasks 4 --strategy layered

# Generate, `bun install`, benchmark, and print a report.
task-bench bench -o /tmp/bench --projects 200 --tasks 4 \
    --strategy random --edge-probability 0.3 \
    --cold-runs 3 --warm-runs 5 --json results.json

# Benchmark an already-generated workspace.
task-bench run -d /tmp/bench --tools omni,turbo --task t3

# Run a whole preset of scenarios and summarize (see "Comprehensive suite").
task-bench suite --preset full --json suite.json --md suite.md

# Resolve a config and print it (no side effects).
task-bench inspect --projects 50 --strategy chain
```

### Key options

| Option | Description |
| --- | --- |
| `-o, --out <dir>` | Root dir to generate the workspace into. |
| `--projects <n>` | Number of projects. |
| `--tasks <n>` | Tasks per project (`t0..tN-1`). |
| `--strategy <s>` | `isolated`, `chain`, `fan-out`, `layered`, or `random`. |
| `--layers <n>` | Layers for the `layered` strategy. |
| `--fanout <n>` | Max upstream deps per project. |
| `--edge-probability <p>` | Edge probability for the `random` strategy. |
| `--log-lines <n>` | Log lines printed per task. |
| `--work <n>` | CPU work iterations per task. |
| `--output-files <n>` | Output files written per task. |
| `--tools <list>` | Comma-separated `omni,turbo,nx,moon`. |
| `--turbo-version` / `--nx-version` / `--moon-version` / `--bun-version` | Pin the version of each tool to install. |
| `--concurrency <n>` | Max parallel tasks, applied identically to every runner (default: CPU count). |
| `--resource-runs <n>` | Dedicated RSS/CPU measurement passes per scenario (`0` disables). |
| `--no-daemon` | Disable each tool's persistent daemon (Turbo, Nx). |
| `--no-chain` / `--no-fan-upstream` | Disable intra/inter-project task deps. |
| `--config <file>` | JSON config to use as a base for overrides. |

## Library

```ts
import { generateWorkspace, runBenchmark, formatReport } from "@omni-oss/task-bench";

await generateWorkspace("/tmp/bench", {
    projects: 100,
    tasksPerProject: 3,
    dependency: { strategy: "layered", layers: 6, fanout: 3 },
});

const result = await runBenchmark("/tmp/bench", { warmRuns: 5 });
console.log(formatReport(result));
```

## Comprehensive benchmark suite

The `suite` command runs a **preset of scenarios** end-to-end (generate →
install → benchmark each) and summarizes them into a combined report, with
`--json` and/or `--md` output.

```sh
# List the built-in presets.
task-bench suite --list
#   quick | shapes | scale | density | daemon | full

# Run a built-in preset and write both machine- and human-readable reports.
task-bench suite --preset full --json suite.json --md suite.md

# Override run parameters for every scenario in the preset.
task-bench suite --preset scale --cold-runs 3 --warm-runs 5 --concurrency 8

# Skip (or tune) resource measurement across the whole suite.
task-bench suite --preset scale --resource-runs 0

# Run only two tools, and keep the generated workspaces around.
task-bench suite --preset shapes --tools omni,turbo --keep -o /tmp/suite

# Run a custom scenario matrix from a JSON file.
task-bench suite --file my-suite.json --md report.md
```

Built-in presets: **`quick`** (tiny smoke test), **`shapes`** (dependency-graph
shapes at fixed scale), **`scale`** (50–600 projects), **`density`** (tasks per
project), **`daemon`** (daemon on vs off), and **`full`** (shapes + scale +
density). A custom preset file looks like:

```jsonc
{
  "name": "my-sweep",
  "displayName": "My Sweep",
  "defaults": {
    "config": { "tasksPerProject": 3, "dependency": { "strategy": "layered" } },
    "run": { "concurrency": 8, "coldRuns": 2, "warmRuns": 3, "resourceRuns": 3 }
  },
  "scenarios": [
    { "name": "small", "displayName": "50 projects", "config": { "projects": 50 } },
    { "name": "large", "displayName": "500 projects", "config": { "projects": 500 } },
    { "name": "large-nodaemon", "config": { "projects": 500 }, "run": { "daemon": false } }
  ]
}
```

Both the suite and each scenario accept an optional `displayName` used as the
label in the JSON output and Markdown report; it falls back to `name` when
omitted (`displayName ?? name`). `name` stays filesystem-safe (it names the
generated workspace directory), while `displayName` is free-form.

The Markdown report contains warm + cold median wall-time matrices (per tool
per scenario) and, when resource measurement is enabled (`resourceRuns > 0`),
warm + cold memory and CPU matrices, followed by the detailed per-scenario
tables; the JSON contains the full resolved config, run options, and per-run
samples for every scenario.

### Scripting it yourself

If you prefer explicit shell control, the equivalent manual loop is:

````bash
#!/usr/bin/env bash
set -euo pipefail

# Use the built CLI (`task-bench`) or the dev entry as below.
BENCH="bun scripts/task-bench/src/cli/index.ts"
OUT=/tmp/task-bench-suite
RESULTS="$OUT/results"
SUITE="$OUT/SUITE.md"
mkdir -p "$RESULTS"
: > "$SUITE"

run() { # run <label> <bench|run> <args...>
  local label="$1"; shift
  echo "### $label" | tee -a "$SUITE"
  echo '```' >> "$SUITE"
  $BENCH "$@" --json "$RESULTS/$label.json" 2>/dev/null | tee -a "$SUITE"
  echo '```' >> "$SUITE"
}

# 1) Dependency-shape sweep (fixed scale): how graph shape affects overhead.
for strat in isolated chain fan-out layered random; do
  run "shape-$strat" bench -o "$OUT/shape-$strat" \
    --projects 120 --tasks 3 --strategy "$strat" \
    --concurrency 8 --cold-runs 2 --warm-runs 3
done

# 2) Scale sweep (layered graph): how overhead grows with graph size.
for n in 50 150 300 600; do
  run "scale-$n" bench -o "$OUT/scale-$n" \
    --projects "$n" --tasks 3 --strategy layered --layers 8 \
    --concurrency 8 --cold-runs 2 --warm-runs 3
done

# 3) Task-density sweep (fixed project count).
for t in 2 5 10; do
  run "density-$t" bench -o "$OUT/density-$t" \
    --projects 120 --tasks "$t" --strategy layered \
    --concurrency 8 --cold-runs 2 --warm-runs 3
done

# 4) Daemon on vs off (reuse one installed workspace).
run "daemon-on"  bench -o "$OUT/daemon" --projects 200 --tasks 3 \
  --strategy layered --concurrency 8 --cold-runs 2 --warm-runs 3
run "daemon-off" run  -d "$OUT/daemon" --no-daemon \
  --concurrency 8 --cold-runs 2 --warm-runs 3

echo "Suite written to $SUITE ; per-scenario JSON in $RESULTS"
# rm -rf "$OUT"   # uncomment to clean up workspaces afterwards
````

Each JSON result contains per-run samples (`durationMs`, `exitCode`,
`executed`), the verified `taskGraphSize`, and `stats` (min/max/mean/median/
stddev) for both scenarios, so you can post-process or chart them however you
like.

### Sample results

Collected on one machine (Linux, 8-way concurrency, 3 tasks/project, layered
unless noted; `warm` = verified 100% cache hit, the discovery + cache-restore
overhead metric). Absolute numbers are hardware-dependent — read the *ratios*.

**Four-runner snapshot** (40 projects × 3 tasks, 120 nodes, layered):

| tool | warm | cold |
| --- | --- | --- |
| omni | **76ms** | 990ms |
| turbo | 135ms | 915ms |
| nx | 434ms | 2.27s |
| moon | 448ms | 1.45s |

The larger sweeps below predate the moon addition (omni/turbo/nx only); rerun
`task-bench suite --preset full` to regenerate them with all four runners.

<!-- SUITE_RESULTS -->

**Dependency-shape sweep** (120 projects × 3 tasks, 360 task-graph nodes):

| strategy | omni warm | turbo warm | nx warm | omni cold | turbo cold | nx cold |
| --- | --- | --- | --- | --- | --- | --- |
| isolated | 292ms | **239ms** | 644ms | 3.27s | 2.52s | 8.22s |
| chain    | 403ms | **231ms** | 1.09s | 5.36s | 4.91s | 9.09s |
| fan-out  | 304ms | **246ms** | 678ms | 3.46s | 2.61s | 8.79s |
| layered  | 252ms | **246ms** | 678ms | 3.21s | 2.63s | 8.71s |
| random   | 284ms | **254ms** | 797ms | 3.59s | 3.12s | 8.72s |

**Scale sweep** (layered, 3 tasks):

| projects (nodes) | omni warm | turbo warm | nx warm | omni cold | turbo cold | nx cold |
| --- | --- | --- | --- | --- | --- | --- |
| 50 (150)  | **106ms** | 134ms | 478ms | 1.34s | 1.14s | 2.81s |
| 150 (450) | 295ms | **287ms** | 764ms | 4.05s | 3.25s | 12.23s |
| 300 (900) | 715ms | **523ms** | 1.17s | 8.62s | 6.44s | 38.68s |

**Daemon on vs off** (200 projects × 3 tasks, 600 nodes):

| mode | omni warm | turbo warm | nx warm | omni cold | turbo cold | nx cold |
| --- | --- | --- | --- | --- | --- | --- |
| daemon on  | 454ms | **361ms** | 915ms | 5.60s | 4.31s | 19.50s |
| daemon off | 455ms | **359ms** | 924ms | 5.62s | 4.32s | 15.27s |

**Observations**

- **Warm overhead:** omni and turbo are close (turbo pulls slightly ahead as
  scale grows); nx is consistently **~2.3–4.7× slower** on warm cache hits. At
  small scale (50 projects) omni is fastest; from ~120 projects up turbo leads.
- **Graph shape matters most for cold:** the `chain` strategy (deep, low
  parallelism) roughly doubles cold time for omni/turbo and gives nx its worst
  warm number, while `isolated`/`layered` (wide, parallel) are cheapest.
- **Scale:** warm overhead grows roughly linearly with task-graph size for all
  three; nx **cold** scales worst (38.7s at 300 projects vs 6–9s for the
  others).
- **Daemons:** on/off barely moves omni/turbo (omni has none; turbo warm is
  unchanged) and nx warm is essentially flat — but nx **cold is faster with the
  daemon off** (~15s vs ~19s) because daemon-mode cold pays a teardown/restart
  each run. So nx's daemon does not materially reduce the per-invocation warm
  overhead this benchmark isolates.

> Reproduce with the script above; exact numbers vary by machine, the relative
> ratios are the takeaway.

## Tool adapters

Each runner is a self-contained adapter (`src/tools/<tool>.ts`) that owns
*everything* tool-specific, so the generator and the other tools stay
decoupled. An adapter declares:

- `supportedVersions` — semver ranges it supports. The version to install is
  configurable (`versions.<tool>` / `--<tool>-version`); if it falls outside
  the supported ranges the harness fails fast with a clear error. External
  tools (omni) instead implement `detectVersion()` and validate the installed
  binary.
- `devDependencies(config)` — the npm packages it needs in the root
  `package.json`.
- `setup(ctx)` — derives and writes its own config from the generated
  `projects` (e.g. `turbo.json`, `nx.json` + `project.json`, `.moon/*.yml` +
  `moon.yml`, `workspace.omni.yaml` + `project.omni.yaml`).
- `run` / `env` / `clearCaches` / `stopDaemon` — the runtime behavior.
- `daemonPids` (optional) — locate this tool's persistent daemon PID(s) so
  resource measurement can include them (omni/moon omit it).

The generator only writes the neutral workspace (projects, `src/index.js`,
`task.mjs`), then hands the project list to each enabled adapter. Adding a new
runner is a single new file plus registering it in `src/tools/index.ts`.

## Notes

- The dependency graph is deterministic for a given `seed`.
- Generated workspaces are self-contained and safe to place anywhere outside
  this repository (each has its own `workspace.omni.yaml`).

### Known limitations

- Numbers are wall-clock process timings on a single machine; absolute values
  vary by hardware, but the harness isolates *relative* discovery/caching
  overhead.
- Cold runs are fully fresh per tool (caches cleared; daemons torn down in
  daemon mode), so cold includes each tool's start-up cost. Warm runs are the
  primary overhead metric.
- Task output is captured (piped) for all runners equally so every tool's
  log-capture path is exercised.
- moon only enables its cache inside a git repository, so generated workspaces
  are committed automatically; benchmarking an existing workspace assumes it is
  already a git repo.
