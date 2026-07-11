# @omni-oss/perf-dashboard

Graphs and dashboards for [`@omni-oss/task-bench`](../task-bench/README.md)
performance-comparison data. Turns the benchmark artifacts published by
[`publish-perf-comparison.yaml`](../../.github/workflows/publish-perf-comparison.yaml)
into a single self-contained report with two views:

- **Cross-tool** — one omni version vs. the other runners in the same runs.
- **Version history** — omni over its own releases, on a normalized slice
  (Linux · `full` preset · resource runs); ineligible versions are dropped and
  shown in a mandatory exclusion panel.

See [`DESIGN.md`](./DESIGN.md) for the full architecture. The pipeline is
`source → normalize → analyze → Chart IR → render`, with pluggable **data
sources** and **renderers**, and a generic, domain-agnostic output IR.

## CLI

```sh
# Full report (all views) from the published GitHub data → self-contained HTML.
perf-dashboard report \
  --source github --repo omni-oss/performance-comparison \
  --renderer html --out ./site

# From local `task-bench suite --json` output (single file or a folder of runs).
perf-dashboard report --source local-fs --path ./data.json --renderer markdown --out .

# Narrow to one view and spotlight a specific omni version.
perf-dashboard report --source github --repo omni-oss/performance-comparison \
  --views cross-tool --spotlight 0.4.1 --renderer html --out ./site

# Inspect the normalized data (no analysis) for debugging.
perf-dashboard inspect --path ./data.json
```

Sources: `local-fs` (`--path`), `github` (`--repo owner/repo`, optional `--ref`,
`--token`/`GITHUB_TOKEN`). Renderers: `json`, `markdown`, `html`.

## Library

```ts
import {
  GitHubDataSource,
  HtmlRenderer,
  run,
} from "@omni-oss/perf-dashboard";

const output = await run({
  source: new GitHubDataSource({ owner: "omni-oss", repo: "performance-comparison" }),
  renderer: new HtmlRenderer(),
});
// output.files -> [{ path: "index.html", content, mime }]
```

`composeReport(runs, opts)` is the pure core (no IO) if you want to bring your
own source/renderer; it returns a `Report` — a serializable description of every
view, chart, and data point.

## Configuration

- `PERF_DASHBOARD_SCENARIO_ALIASES` — JSON object of `{ alias: canonical }`
  scenario-name pairs, merged over the checked-in constant map (env wins). Used
  by the version-history view to bridge scenario renames across releases.
