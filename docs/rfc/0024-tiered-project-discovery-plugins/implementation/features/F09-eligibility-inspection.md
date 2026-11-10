# F09: Cache-eligibility and provenance inspection (CLI/MCP)

**Type:** feature · **Size:** M · **Depends on:** B04, F05 (F07 for field provenance)

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

## Acceptance

- Snapshot tests for each status variant in CLI JSON and MCP.
- Redaction applied; no file contents in output.
- Disabling output channels doesn't change execution.
