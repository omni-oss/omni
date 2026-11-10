# R06: Explicit exact/pattern inputs in planner and shared API

**Type:** refactor · **Size:** M · **Depends on:** none · **Unblocks:** F03, F04

## Problem

Task requests are undifferentiated strings treated as globs. Internal callers synthesize
`"*"` and `"**"` to mean "all tasks": cache stats/prune/staleness
(`omni_cache/src/cache/impls/hybrid.rs` ~L157, ~L1182) and `ProjectHasher::hash`
(`omni_context/src/project_hasher.rs` ~L40). A real task named `*` or `build*` cannot be
selected literally, and "all tasks" is indistinguishable from a user pattern.

## Why it matters

Standalone: literal task names with glob characters become selectable; internal
"all tasks" becomes intentional. For plugins: inferred names are raw ecosystem strings
(npm scripts can contain `*`, `:`, etc.) and must be addressable exactly.

## RFC references

- [§7.4](../../007.03-task-scoping-aliases-and-selectors.md#74-host-owned-task-scoping-and-aliases)
- [§17.8 Selectors](../../017.00-review-findings-and-prerequisites.md#178-scoped-object-references-and-alias-aware-selectors)
- [§13.2 rows for `omni_execution_plan`, `omni_api`, `ProjectHasher`](../../013.00-implementation-plan.md#132-existing-crates-to-change)

## Code starting points

- `crates/omni_execution_plan/src/call.rs` (`Call::new_tasks`), `filter.rs`,
  `execution_plan_provider.rs`, `traits.rs`
- `crates/omni_api/src/operations/task.rs` (shared request, currently only `tasks`)
- `crates/omni_cache/src/cache/impls/hybrid.rs`, `crates/omni_context/src/project_hasher.rs`
- `crates/omni_cli_core/src/commands/run.rs` (positional tasks required)

## Work

1. Introduce a typed selector input: `Exact(name)` and `Pattern(glob)` (plus whatever
   "all tasks" internal helper is clean). Keep the two as separate code paths.
2. Migrate **every** internal caller of `"*"`/`"**"` to an explicit pattern request.
3. Public API/CLI behavior unchanged in this package: positional CLI/MCP inputs keep
   their current meaning (glob) until F04 flips them to exact.
4. Each surface retains its existing no-selector default.

## Out of scope

- Alias-aware matching, new CLI flags, exact-selector semantics (F04), scoped
  references (F03).

## Acceptance

- Behavior-preserving: all existing run/hash/cache tests pass.
- Cache stats/prune/staleness and `ProjectHasher::hash` operate via the explicit
  pattern input, verified by tests that would fail if a literal `*` exact name were
  treated as run-all.
- `omni_api` request type can carry both selector kinds (not yet surfaced to users).
