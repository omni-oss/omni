# F09: Cache-eligibility and provenance inspection (CLI/MCP)

**Type:** feature · **Size:** M · **Depends on:** B04, F05 (F07 for field provenance)
**Status:** slices 1 (API operation), 2 (CLI) and 3 (MCP) done and merged ([#160](https://github.com/omni-oss/omni/pull/160)). Field provenance for `cache.enabled` is added by F07.5 ([#203](https://github.com/omni-oss/omni/pull/203)); other fields wait for P12.

## Goal

Expose, through existing CLI/MCP inspection and dry-run output, each task's typed
cache-eligibility status (with deciding source, demotion cause) and, once F07 lands,
the provenance of effective values. One status type feeds the aggregated informational
note, CLI/JSON and MCP.

## RFC references

- [§8.3 typed status](../../008.02-execution-cache-semantic-identity.md#cache-enabled-tasks-and-uncached-dependencies)
- [§9 observability](../../009.00-failures-security-and-observability.md)
- [§13.6 definition of done: CLI and MCP expose provenance and cache status](../../013.00-implementation-plan.md#136-definition-of-done-for-v1)

## Code starting points

- `crates/omni_cli_core/src/commands/project.rs`, `run.rs` (dry-run), `hash.rs`
- `crates/omni_mcp_core` _(locate project/task inspection tools)_
- `crates/omni_messages`, `omni_notice` (reuse notices; no new channel)

## Work

1. Serialize the B04/F05 status in JSON outputs and MCP tool results; stable field names.
2. Human-readable rendering: task names and tiers only; paths/values redacted per
   existing redaction rules (RFC 0022/0026).
3. Aggregated single informational note per run (already in B04); per-task detail on
   request.
4. Schema/docs updates.
5. List explicit cache contradictions without applying F05.2's planning error
   to the inspection command. Report the checked plan and any conditions that
   were not evaluated.

## Acceptance

- Snapshot tests for each status variant in CLI JSON and MCP.
- Inspection can report both declaring files for an authored contradiction
  without failing the inspection or executing task commands.
- Redaction applied; no file contents in output.
- Disabling output channels doesn't change execution.

## Slices

1. **API operation (done).** `omni_task_executor::inspect_cache_eligibility` plans a
   selection and reports each task's `CacheStatus`, the tier that decided
   `cache.enabled`, and every explicit contradiction with both declaring files
   (workspace-relative). It runs nothing, evaluates no `enabled` expression, loads no
   environment and never fails on a contradiction. Statuses come from the executor's
   `derive_status`; `enabled` classification shares `static_enabled` with
   `TaskContext::is_enabled`. Tasks with a template `enabled` are judged as enabled
   and listed in `unevaluated_conditions`. The operation loads a fresh definitions-only context (`Context::into_loaded_definitions_only`), so no environment value is evaluated and no command substitution can run. Exposed as `OmniApi::task_cache_inspect`
   (request: `TaskRunRequest`, selection fields only). `--no-cache` is not modelled, so
   `write-only-no-cache` is not reported yet.
2. **CLI (done).** `omni cache eligibility -t <task>` (also `--task-pattern`, `-p`, `--project-pattern`, `--dir`, `-m`, `--ignore-dependencies`, `--json`). Text output names tasks, statuses, tiers and the declaring files; `--json` prints the inspection. A JSON test pins every status variant's shape.
3. **MCP (done).** Read-only tool `cache_eligibility` (`tasks`, `task_patterns`, `project`, `project_patterns`, `dir`, `meta`, `ignore_dependencies`) returning `tasks`, `contradictions` and `unevaluated_conditions` with the same field names as the CLI JSON. Output carries task names, tiers and workspace-relative declaring files only; no file contents.
