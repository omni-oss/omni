# F04: exact selectors plan

**Status:** approved and merged in [#134](https://github.com/omni-oss/omni/pull/134) (task selectors for `run` and MCP `task_run`), together with [F04.2](F04.2-hash-cache-selectors-plan.md) and [F04.3](F04.3-project-filter-plan.md).
**Kind / risk:** Medium / Medium. User-visible, pre-v1 breaking change to how `omni run` reads task arguments; no cache, capability or persisted-data change.

## Goal

Positional task names become **exact**; a repeatable `--task-pattern` flag (and MCP field) selects by glob. Pattern-only, exact-only and mixed requests union and deduplicate. See [F04](F04-exact-selectors-and-task-patterns.md) for the full requirement.

## What exists already

- The planner and `omni_api` already split the two paths: `Call::exact_tasks` / `TaskSelector::{Exact,Pattern,All}`, `TaskRunRequest::exact_tasks`, and filter tests for literal `*`.
- The surfaces still send everything down the pattern path: `omni run` uses `Call::pattern_tasks(&command.task)`, and the MCP task tool passes `exact_tasks: vec![]`.
- `hash` and `cache` CLI commands take `task: Vec<String>` and pass it through separately; each needs its own check.

## Approach

1. **CLI.** In `run` (and `hash`, `cache` where they take tasks), positional `task` builds exact selectors; add `--task-pattern <glob>` (repeatable) for patterns. Keep each command's existing no-selector default.
2. **MCP.** `TaskRunParams.tasks` becomes exact; add `task_patterns`. Update the JSON schema and tool description.
3. **Missing exact task.** An exact selector that matches no task in the project/directory scope fails planning before metadata/affected narrowing, as the requirement says; a later filter exclusion is not an error. If the name contains glob characters and a pattern would have matched, add the hint `use --task-pattern 'build*'`.
4. **Docs and completions.** CLI help, `declspec` completions, MCP schemas, the changelog migration note (`omni run 'build*'` becomes `omni run --task-pattern 'build*'`), and any workflow or docs that use globs in positionals (to be found with a repo search).
5. Aliases stay out of executable node maps (F03/R05 territory).

## Open questions (assumptions to confirm)

- **Hard break or deprecation period?** The requirement says migration diagnostics, not a compatibility mode. Assumed: hard break with the hint.
- **Where the missing-exact-task check lives.** Assumed in the planner, which both CLI and MCP share. To be confirmed by reading `execution_plan_provider.rs`.

## Verification and undo

- Tests: exact, pattern-only, mixed and duplicate requests; escaped pattern fixtures; literal `*`/`**` names; the migration hint; CLI and MCP parity; existing integration tests moved to the new syntax.
- Undo: revert; no stored data changes. The only residual risk is scripts that relied on positional globs, which is what the migration note is for.
- Checks: scoped tests, clippy, fmt for the touched crates, plus API/CLI/MCP test compilation.

## Not in scope

Alias-aware matching, plugin-inferred task names, and the F03.3 root lowering.
