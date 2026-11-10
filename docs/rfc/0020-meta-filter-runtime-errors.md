# RFC 0020: Surface meta filter runtime errors

- **Status:** Implemented <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-02
- **Base branch:** `main`
- **Affected crates/files:** `omni_expressions` (`evaluator.rs`), `omni_execution_plan` (`filter.rs`, `execution_plan_provider.rs`), `omni_task_executor`, `omni_cli_core` (`commands/common_args.rs`), `omni_api` (`operations/task.rs`), `omni_mcp_core`, `tests/omni-tests`

## 1. Summary

`--meta <CEL expression>` (CLI, MCP `task_run`, and the other operations that
accept a meta filter) selects tasks by evaluating a CEL expression against each
task's `meta` map. Today every runtime failure of that expression is swallowed
and treated as `false`: the task is silently excluded. A typo, a wrong type, or
a task without the referenced key all look the same as "does not match", and
the user gets an empty or incomplete task set with no explanation.

This RFC makes runtime evaluation errors visible. By default, evaluation
errors abort planning with one aggregated diagnostic that names the failing
tasks (a bounded sample per cause plus totals), the expression, the underlying
CEL error, and the meta data the expression was evaluated against. A `--meta-on-error` option keeps the old
"exclude" behaviour as an explicit, still-reported opt-in.

## 2. Motivation / Problem

The swallowing happens in one place,
`crates/omni_execution_plan/src/filter.rs`:

```rust
if let Some(mf) = &self.meta_filter
    && !mf.coerce_to_bool(&get_meta(node)?).unwrap_or(false)
{
    return Ok(false);
}
```

`coerce_to_bool` already returns `Result<bool, Error>`; `unwrap_or(false)`
discards it. (`filter_tasks` / `filter_tasks_cloned` in `TaskFilterExt` do the
same thing for the whole filter.) This was intentional: a filter such as
`kind == "e2e"` should simply skip tasks that have no `kind`, so users never
had to guard against missing keys. In practice it does not work:

- Parse errors are reported, but runtime errors are not, so `--meta 'kind == 1 +
  "a"'` parses fine and then matches nothing, silently.
- A typo (`--meta 'knd == "e2e"'`) is "no such attribute" for every task and
  yields an empty plan indistinguishable from "no task is e2e".
- Type mismatches (`--meta 'priority > "high"'` where `priority` is an int) are
  hidden.
- Agents using the MCP `task_run` tool cannot self-correct. They see "no tasks
  matched" and retry blindly.
- With hundreds of tasks, even when a user suspects the filter, nothing says
  which task misbehaved or what its meta looked like.

### Reproduction

```text
$ omni run test --meta 'kind == "e2e"'      # only some tasks define `kind`
$ omni run test --meta 'knd == "e2e"'       # typo, same output
No tasks matched the given filters.
```

## 3. Goals / Non-goals

### Goals

- A runtime error in a meta filter is never silently turned into `false`.
- The diagnostic tells the user _which task_ failed (project, task name, dir),
  _what_ failed (expression and CEL error), and _with what data_ (the meta
  variables the expression saw).
- The same diagnostic is available, structured, to MCP clients.
- Users can still opt into tolerant filtering, but tolerated errors are
  reported rather than hidden.
- Provide a documented way to write filters over optional keys.

### Non-goals

- Changing CEL semantics or the `omni_expressions` language surface.
- Changing parse-error handling (already reported).
- Applying this to other expression uses (e.g. `enabled`/`if` Tera expressions);
  those can adopt the same error type later.

## 4. Design

### 4.1 Behaviour

| Situation                                         | Before            | After (default `fail`)           |
| ------------------------------------------------- | ----------------- | -------------------------------- |
| Expression evaluates to true / false              | include / exclude | unchanged                        |
| Expression references a missing meta key          | exclude silently  | **error**                        |
| Type error, bad function call, overflow ...       | exclude silently  | **error**                        |
| Meta conversion error (`into_expression_context`) | already error     | unchanged, now with task context |
| Non-bool result (including `null`)                | truthy / falsy    | **error** (4.5)                  |
| Parse error                                       | error             | unchanged                        |

Evaluation continues over all tasks and failures are aggregated with bounded
memory (section 4.4); planning then returns one error.

### 4.2 Optional keys

Failing on a missing key is the correct default (it catches typos) but must not
make optional-key filters impossible. Meta keys are top-level
variables, and CEL's `has()` only works on field selection, not on a missing
variable, so today there is no way to test for an optional key. To make optional
keys practical, the evaluation context additionally exposes the whole map as
`meta`:

```text
--meta 'has(meta.kind) && meta.kind == "e2e"'
--meta '"kind" in meta && meta.kind == "e2e"'
```

Top-level variables continue to work unchanged. `meta` is always
injected, as an empty map for tasks with no meta configuration, so
`has(meta.kind)` is `false` rather than an undeclared-reference error for them
(today such tasks get a context with no variables at all). It is skipped only if
the task's meta already defines a key named `meta` (the user key wins).

### 4.3 `--meta-on-error`

```text
--meta-on-error <fail|exclude>    default: fail
```

- `fail`: abort planning with the error below.
- `exclude`: previous behaviour (task is dropped), but every error is
  reported as a warning naming the task.

The same field is added to the MCP `task_run` filters as `meta_on_error`
(enum, optional) and to `omni_api` task filter parameters.

### 4.4 Bounded aggregation

`N` is configurable with `--meta-error-samples <N>` (MCP: `meta_error_samples`),
default `3`. `0` reports counts and causes only, with no task samples. `N` is an unsigned integer
(`usize`), so negative or non-numeric values are rejected by the parser. There is
no upper limit: choosing a size that fits memory and output is the user's
responsibility, and the value is never adjusted silently.

In `fail` mode the filter loop keeps evaluating after a failure so one run
reports the whole problem, without memory growing with the task count:

- Failures are grouped by CEL error message. Each group stores a total `count`.
- Each group retains at most `N` detailed samples (default 3, see below). At
  most 10 groups are retained; failures with further distinct causes only
  increment an `other` counter. A failure past a limit only increments counters.
- A meta snapshot is built only for a retained sample, truncated to 20 keys and
  256 characters per value. Dropped failures allocate nothing.
- Per-task results are not accumulated: each is evaluated and discarded.
- Samples are the first failing tasks in plan evaluation order (graph node
  order, which is deterministic), so output is stable between runs.
- The limits live in one `MetaFilterErrorLimits { samples_per_cause, max_causes }`
  struct. `samples_per_cause` is set from the option (default 3); `max_causes`
  is a named constant (10). Making it configurable later is a one-line change.

Memory is O(10 * N), independent of the number of tasks. Output: `knd` undeclared for 500
tasks (3 shown, 497 more).

### 4.5 Non-boolean results

`coerce_to_bool` currently maps any value to a truthiness (`"x"` is true, `0` is
false, any list or map is true). That hides mistakes such as `--meta owner`
(true for every task with a non-empty owner). The filter must evaluate to a
`bool`. Any other result, including `null`, is an error:

```text
cause: filter must evaluate to a bool, got string
```

Users write the comparison explicitly (`owner != ""`, `size(tags) > 0`). The
error is a normal evaluation failure: it goes through the same aggregation and
`--meta-on-error` handling as any other runtime error. `Evaluator::coerce_to_bool`
is renamed `evaluate_bool` since it no longer coerces; the benchmark is updated.

## 5. What it looks like

Cause strings are the `cel` error messages verbatim (for example
`Undeclared reference to 'knd'`); the exact wording in these examples is
illustrative and tests assert on substrings.

### 5.1 CLI, missing key / typo

```text
$ omni run test --meta 'knd == "e2e"'
error: meta filter failed to evaluate for 500 task(s)

  filter:  knd == "e2e"
  cause:   Undeclared reference to 'knd'   [500 tasks]

    web#test   apps/web   meta: { "kind": "unit", "owner": "frontend" }
    api#test   apps/api   meta: { "kind": "e2e" }
    docs#test  apps/docs  meta: {}
    ... and 497 more

  hint: `knd` is not defined for this task. Did you mean `kind`?
        For optional keys use has(meta.knd). To skip failing tasks instead,
        pass --meta-on-error exclude.
```

### 5.2 CLI, type error

```text
$ omni run test --meta 'priority > "high"'
error: meta filter failed to evaluate

  filter:  priority > "high"
  cause:   no such overload: int > string

  task:     api#test
  project:  api  (apps/api)
  meta:     { "priority": 2 }
```

### 5.3 `--meta-on-error exclude`

```text
$ omni run test --meta 'kind == "e2e"' --meta-on-error exclude
warn: meta filter failed for 2 task(s); they were excluded (see --meta-on-error)
  web#test: Undeclared reference to 'kind'   meta: {}
  docs#test: Undeclared reference to 'kind'  meta: {}
Running 1 task ...
```

### 5.4 MCP

An invalid-parameters style error whose `data` is structured so agents can
locate the task without parsing text:

```json
{
    "code": -32602,
    "message": "meta filter failed to evaluate for 500 task(s)",
    "data": {
        "kind": "meta_filter_evaluation",
        "filter": "knd == \"e2e\"",
        "total": 500,
        "groups": [
            {
                "cause": "Undeclared reference to 'knd'",
                "count": 500,
                "samples": [
                    {
                        "project": "web",
                        "task": "test",
                        "project_dir": "apps/web",
                        "meta": { "kind": "unit", "owner": "frontend" }
                    },
                    {
                        "project": "api",
                        "task": "test",
                        "project_dir": "apps/api",
                        "meta": { "kind": "e2e" }
                    },
                    {
                        "project": "docs",
                        "task": "test",
                        "project_dir": "apps/docs",
                        "meta": {}
                    }
                ],
                "omitted": 497,
                "hint": "`knd` is not defined for this task. Did you mean `kind`? For optional keys use has(meta.knd)."
            }
        ]
    }
}
```

## 6. Implementation

### 6.1 `omni_expressions`

`Evaluator::coerce_to_bool` is renamed `evaluate_bool` (4.5) and returns `Err`
for non-bool results. Its doc comment, which claims errors are coerced
("including errors"), is rewritten.

Add a public classification so callers never match on `cel` types or message
text:

```rust
pub enum ErrorClass {
    MissingReference { name: String, is_call: bool },
    MissingKey { key: String },
    TypeMismatch { detail: String },
    NonBool { got: String },
    Other,
}

impl Error { pub fn class(&self) -> ErrorClass; }
```

It is derived from `cel::ExecutionError` variants: `UndeclaredReference` ->
`MissingReference`, `NoSuchKey` -> `MissingKey`, `NoSuchOverload` /
`UnexpectedType` / `ValuesNotComparable` / `UnsupportedBinaryOperator` ->
`TypeMismatch`, our non-bool error -> `NonBool`, the rest -> `Other`. Only this
crate depends on `cel::ExecutionError`, so `cel` upgrades touch one place.
`is_call` is set when the undeclared name is invoked as a function or method in
the expression (checked against the parsed expression, not the message), since
`UndeclaredReference` is also raised for unknown functions such as `startWith`.
The `kind()` accessor is no longer `#[allow(unused)]`.

Exposing `meta` must not add copying. `MetaConfiguration::to_expression_context(&self)`
builds the context through `Context::add_map_with_root`. Each value is converted
once with `v.clone().into_json()`, then the root `meta` map and the top-level
entries are registered from that conversion. `cel::Value` maps, lists, and strings
are `Arc`-backed, so these registrations are reference-count bumps, not deep copies. Net allocation is
lower than today. Caching the context per task is out of scope.

### 6.2 `omni_execution_plan`

Add a typed error variant:

```rust
FilterErrorInner::MetaFilter(MetaFilterError)

pub struct MetaFilterError {
    pub filter: String,
    pub groups: Vec<MetaFilterGroup>,
    pub total: usize,
    pub other: usize,
}

pub struct MetaFilterGroup {
    pub cause: String,
    pub count: usize,
    pub samples: Vec<MetaFilterFailure>,
}

pub struct MetaFilterFailure {
    pub project: String,
    pub task: String,
    pub project_dir: PathBuf,
    pub error: omni_expressions::Error,
    pub meta: Option<serde_json::Value>,   // None if meta conversion failed
}
```

`DefaultTaskFilter` replaces `.unwrap_or(false)` with a match.
`should_include_task` returns the per-task error, or, when
`collect_meta_errors(limits)` is set, records the failure in a bounded
collector and excludes the task. `DefaultExecutionPlanProvider` applies
`MetaOnError` after filtering: `fail` returns the aggregate error, `exclude`
emits a warning.

Where the filter runs (traced in `execution_plan_provider.rs` and
`omni_core::task_execution_graph`):

- The plan graph calls the filter closure once per graph node, in node-index
  order (graph build order), as `Fn(&Node) -> Result<bool, eyre::Report>`, and
  short-circuits on the first `Err`. Aggregation therefore cannot return `Err`
  from the closure. In `fail` mode the provider's closure records the failure
  in a bounded collector (4.4, behind a `RefCell` because the closure is `Fn`),
  returns `Ok(false)`, and after the plan call returns the aggregate if the
  collector is non-empty. The same collector wraps the direct loops (`Command`
  call and `get_execution_plan_ignored_dependencies`), which iterate projects
  and `OrderedMap` tasks in a deterministic order.
- Dependencies are never filtered. They are pulled in by graph reachability, so
  the meta filter is not evaluated on them and cannot fail for them.
- The dependents filter (`tf_dependents`) is built with no meta, project or dir
  filter (task names only), so it also cannot raise meta errors.
- The task, project and dir matchers run before the meta filter, so it is only
  evaluated for tasks that already passed them.

Consequence: a meta error can only come from tasks selected by the user's
task/project/dir filters, and it names exactly those tasks.

`TaskFilterExt::filter_tasks*` currently use `unwrap_or(false)` for the whole
filter. They are `#[allow(unused)]` / test-only; they are changed to propagate
`Result` so the same swallowing cannot reappear through them.

The `meta` data in the diagnostic is rendered through the same JSON conversion
already used to build the CEL context and truncated (values > 256 chars, > 20
keys) so a large meta cannot flood the output.

### 6.2.1 Hint generation

Hints are produced in `omni_execution_plan` from `ErrorClass` plus the failing
task's meta; never from message text. The same `hint: String` feeds the CLI
output and MCP `data.hint`, so they cannot drift.

| `ErrorClass`                   | Hint                                                                                                                                   |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `MissingReference` (variable)  | `` `name` is not defined for this task. For optional keys use `has(meta.name)`. `` plus `Did you mean `kind`?` when a close key exists |
| `MissingReference` (`is_call`) | `` `name` is not a known function. `` plus the closest built-in name                                                                   |
| `MissingKey { key }`           | `` `key` is missing in a nested map. Use `has(meta.a.key)`. ``                                                                         |
| `TypeMismatch { detail }`      | `Operand types differ: ` + the CEL detail (e.g. `int > string`)                                                                        |
| `NonBool { got }`              | ``Filter must be a bool, got `got`. Compare explicitly, e.g. `owner != ""` or `size(tags) > 0`.``                                      |
| `Other`                        | none                                                                                                                                   |

"Did you mean" uses Jaro-Winkler similarity from `strsim` (threshold 0.8) between the
missing name and the task's meta keys, returning at most one suggestion. The
hint is computed once per cause group, from the first retained sample's meta, so
cost and memory stay bounded; the suggestion therefore reflects that sample's
keys and may not hold for every task in the group.

`strsim` 0.11 is already in `Cargo.lock` (transitively via `clap`), so it adds
no new crate to the build. It is stable and feature-complete (last release
2024-04) and is what `clap` uses for its own suggestions. The call is isolated in
one function (name + keys -> optional suggestion), so replacing it later is a
local change. It is declared as a workspace dependency used by `omni_execution_plan`.

### 6.3 Surfaces

- CLI: `--meta-on-error` in `RunArgs` and `ExecutionConfigBuilder`; warnings
  routed through existing tracing; error printed by the existing `eyre` report
  with the layout above.
- `omni_api`: `meta_on_error: Option<MetaOnError>` next to `meta`, flowing into
  `omni_task_executor::config`.
- MCP: schema field plus structured error `data` (5.4).
- Docs: command reference and a "Writing meta filters" section with the optional
  key patterns.

### 6.4 Meta is non-sensitive

Meta is non-sensitive by contract: it describes tasks (kind, owner, priority)
and must not hold secrets. Diagnostics therefore print the meta snapshot as is,
with no redaction, subject only to the truncation limits in 4.4. The meta
documentation states this explicitly.

## 7. Compatibility

Filters relying on truthiness of non-bool values (`--meta owner`) also break and
must become explicit comparisons.

This is a behaviour change: filters that previously "worked" only because errors
were swallowed (e.g. `kind == "e2e"` across tasks that lack `kind`) now fail.
Migration is one of `has(meta.kind) && meta.kind == "e2e"` or
`--meta-on-error exclude`. This is accepted because the previous behaviour
cannot be distinguished from a broken filter. Flag as `fix:` with a changelog
note under breaking behaviour. Valid filters over complete meta are unaffected.

## 8. Alternatives considered

- **Keep swallowing, add a warning only.** Simple, but still produces a plan
  that silently runs the wrong tasks; agents and CI would not stop.
- **Treat only "undeclared reference" as false, fail on the rest.** Preserves
  the optional-key convenience but keeps typos silent, the most common mistake.
- **Per-task opt-in via expression helpers only (no flag).** Doesn't give a
  migration path for existing scripts.

## 9. Decisions

- `fail` is the default.
- Meta is non-sensitive by contract; diagnostics print it without redaction (6.4).
- The `meta` map variable ships in this change; it is the only way to test optional keys.
- Failures are aggregated with bounded memory (4.4); samples per cause default to 3 (`--meta-error-samples`), at most 10 causes retained.
- The 10-cause limit stays hardcoded: there is no clear use case for tuning it
  (it only guards against many distinct messages), but it lives in
  `MetaFilterErrorLimits` so it can be exposed later.
- Samples are the first failing tasks in plan order.
- `strsim` provides "did you mean".
- MCP warnings for `exclude` mode are out of scope for now; MCP clients get
  them only via logs.
- Existing tests that depended on swallowed errors are fixed on this branch as
  the implementation surfaces them.

## 10. Testing plan

Existing tests that exercised swallowed errors (for example `filter.rs` tests
where a filter over a missing key expects exclusion, and `run-filters.e2e.spec.ts`
cases that rely on truthiness or missing keys) are updated to the new contract.

### 10.1 Unit tests

`omni_expressions` (`evaluator.rs`):

- `evaluate_bool` returns `Ok(true)` / `Ok(false)` for bool results.
- Non-bool results (`1`, `"x"`, `[]`, `{}`, `null`, duration, timestamp, bytes)
  return `Err` naming the actual type.
- Runtime errors (`1 + "a"`, undeclared variable, missing map key, bad overload,
  integer overflow) return `Err`; parse errors still return `Err` from `parse`.
- `has(meta.k)` over a missing key is `Ok(false)`.
- `Error::class()` maps undeclared reference, missing key, type mismatch, and
  non-bool results to the right `ErrorClass`; `is_call` is true for
  `startWith(x)` and false for a bare variable.

`omni_configurations` (`meta_configuration.rs`):

- `into_expression_context(&self)` exposes every top-level key and `meta`.
- `meta.k` and `k` evaluate identically, including nested maps and lists.
- A task key named `meta` wins and `meta` is not injected.
- A task with no meta configuration still gets an empty `meta` map, so
  `has(meta.kind)` is `false` for it.
- The meta is not consumed or cloned by the call (`&self`).

`omni_execution_plan` (`filter.rs`, `execution_plan_provider.rs`):

- Missing key, type error and non-bool result each yield a `FilterError` that
  carries project, task, project dir, filter, cause, and meta snapshot.
- Task, project and dir pre-filters short-circuit: a failing meta filter is not
  evaluated for tasks they already excluded.
- Meta that cannot be converted to a context reports the same task context.
- Aggregation: 500 failing tasks with one cause give `total = 500`, one group,
  `count = 500`, and `samples.len() == 3`.
- Samples default to 3; `N = 1`, `N = 0` (no samples), and a large `N` (for
  example 1000) behave as specified; negative and non-numeric values are rejected
  by the parser.
- More than 10 distinct causes retain 10 groups and count the rest in `other`.
- Hints: typo `knd` suggests `kind`; no close key gives the generic `has(meta.x)`
  hint; unknown function `startWith` suggests `startsWith`; type mismatch, nested
  missing key, and non-bool each produce their row from 6.2.1; `Other` has none.
  The hint is computed once per group.
- Retained meta snapshots are truncated to 20 keys and 256 characters per value;
  dropped failures build no snapshot (asserted via a counting meta source).
- `exclude` drops failing tasks and returns one warning per failing task;
  `fail` returns the aggregate error and no plan.
- Dependencies and dependents are never evaluated against the meta filter: a
  dependency task whose meta would fail the filter does not produce an error.
- Samples are the first failing tasks in graph node order, stable across runs.
- `filter_tasks*` propagate errors instead of swallowing them.
- Valid boolean filters over complete meta give identical results to before.

`omni_cli_core`, `omni_api`, `omni_mcp_core`:

- `--meta-on-error` and `--meta-error-samples` parse, default to `fail` and `3`,
  and reject invalid values.
- API and MCP parameters (`meta_on_error`, `meta_error_samples`) deserialize and
  flow into the execution config; MCP schema snapshots are updated.
- The MCP error maps to the structured payload in 5.4 (`kind`, `filter`, `total`,
  `groups[].cause|count|samples|omitted`, `hint`).

### 10.2 E2E tests

In `tests/omni-tests/src/meta-filter-errors.e2e.spec.ts`, using the existing
harness and fixture workspaces (three projects: `web` with `kind: unit`,
`api` with `kind: e2e`, `docs` with no meta):

- Typo (`--meta 'knd == "e2e"'`): non-zero exit, stderr has the filter, cause,
  `[3 tasks]`, the task names and dirs, meta snapshots and the hint; no task runs.
- Missing key on some tasks (`--meta 'kind == "e2e"'`): fails naming `docs`;
  the optional form `has(meta.kind) && meta.kind == "e2e"` runs only `api`.
- Type error (`--meta 'priority > "high"'`): non-zero exit with the cause.
- Non-bool result (`--meta kind`): non-zero exit, `got string`.
- `--meta-on-error exclude`: exit zero, warning lists the excluded tasks, the
  run executes the remaining matching tasks.
- `--meta-error-samples 1` and `0` change the number of listed tasks but not the
  exit code or the total.
- A larger generated workspace (for example 200 tasks) shows the capped output
  with the correct "and N more" count.
- A valid filter over complete meta behaves exactly as before.
- MCP e2e: `task_run` with a failing filter returns the structured error with
  the expected `data`, and `meta_on_error: "exclude"` succeeds.
- Existing `run-filters.e2e.spec.ts` stays green after its fixtures are updated.

### 10.3 Non-functional

- Extend `omni_expressions_benchmarks.rs` to cover `evaluate_bool` and context
  construction with and without `meta`, and confirm no regression versus today.

## 11. Acceptance criteria

- `omni_expressions`: runtime error (`1 + "a"`, missing variable) returns `Err`
  from `evaluate_bool`; `has(meta.k)` over a missing key is `false`.
- `omni_execution_plan`: missing key yields `FilterError` carrying project,
  task, dir, filter, and meta; `exclude` drops and reports.
- `filter_tasks*` propagate errors.
- Aggregation: 500 failing tasks retain at most `N` samples per cause (default 3), and the correct total is reported; `N` is overridable and `0` omits samples.
- A meta that cannot be converted to a context reports the same task context.
- CLI integration: typo filter exits non-zero with the layout in 5.1; with
  `--meta-on-error exclude` exits zero and prints the warning.
- MCP: `task_run` with a failing filter returns the structured error in 5.4.
- Results `1`, `"x"`, `[]`, `{}` and `null` error with the type name; bools unchanged.
- Valid boolean filters over complete meta behave exactly as before.
