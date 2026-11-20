# F04: Exact positional task selectors and `--task-pattern`

**Type:** feature (user-visible, pre-v1 migration) · **Size:** M
**Depends on:** R06 (hard), F03 (for alias-aware matching; exact/pattern split can ship first)

## Status

Implemented, pending PR review, under the approved [plan](F04-selector-plan.md) (hard break,
planner-level check). Medium risk: no cache, capability or stored-data change.

- `omni run` positional tasks are exact; `--task-pattern <glob>` (repeatable) selects by glob;
  at least one of the two is required. The MCP `task_run` tool does the same with `tasks`
  (exact) and `task_patterns`.
- The planner rejects an exact selector that names no task in the project and directory scope
  (`plan.exact_task_not_found`), on both the dependency and ignore-dependencies paths. It runs
  before the meta and affected narrowing, so a task those later exclude is not "missing". A
  name that looks like a glob gets a hint to use `--task-pattern`.
- **Migration:** `omni run 'build*'` becomes `omni run --task-pattern 'build*'`. A script that
  passed a glob positionally now fails with that hint instead of running a set of tasks.
- Not changed: `hash` and `cache` take their own task lists (`-t`, project hash/stats), which
  are globs for internal all-tasks callers and are a separate surface. Aliases stay out of
  the executable node maps. Whether those surfaces should also be exact is a follow-up.

## Goal

Positional tasks become **exact** names. A repeatable `--task-pattern` flag (and matching
MCP field) selects by glob over canonical names (and aliases once F03/R05 alias support
exists). Pattern-only and mixed requests are allowed; results are unioned and
canonically deduplicated inside the existing filter/plan pipeline.

## Why it matters

Standalone: literal task names with glob characters work; scripts that relied on glob
behavior in positionals get an explicit migration path. For plugins: inferred names can
contain `*`, `:`, `/`.

## RFC references

- [§7.4](../../007.03-task-scoping-aliases-and-selectors.md#74-host-owned-task-scoping-and-aliases)
- [§17.8 Selectors](../../017.00-review-findings-and-prerequisites.md#178-scoped-object-references-and-alias-aware-selectors)
- [§13.2 `omni_cli_core`/`omni_mcp_core`/`omni_api` rows](../../013.00-implementation-plan.md#132-existing-crates-to-change)

## Code starting points

- `crates/omni_cli_core/src/commands/run.rs`, `commands/exec.rs`, `commands/hash.rs`,
  `commands/cache.rs`
- `crates/omni_mcp_core` task tools _(locate request schemas)_
- `crates/omni_api/src/operations/task.rs`
- `crates/omni_execution_plan` (filters)

## Work

1. Surface the R06 selector split: positional = `Exact`, `--task-pattern` = `Pattern`.
2. Each exact selector must exist in the project/directory scope **before**
   metadata/affected narrowing; later filter exclusion is not a missing-task error.
3. Preserve backslashes in references (no extra decoding); literal `"*"`/`"**"` exact
   names are not run-all.
4. Keep each command's existing no-selector default.
5. Update CLI help, docs, completions (`declspec`), MCP schemas, and the changelog with a
   migration note: `omni run 'build*'` → `omni run --task-pattern 'build*'`.
6. Aliases stay out of executable node maps (alias lowering is F03/R05 territory).

## Acceptance

- Exact, pattern-only, mixed, duplicate (union/dedup) requests.
- Escaped pattern fixtures; literal-wildcard task names.
- CLI/MCP parity tests; migration diagnostics when a positional contains glob
  characters and no exact task matches (helpful hint to use `--task-pattern`).
- Existing integration tests updated to the new syntax.
