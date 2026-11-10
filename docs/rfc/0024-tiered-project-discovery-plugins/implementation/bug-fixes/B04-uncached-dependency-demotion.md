# B04: Uncached dependencies feed a zero hash into cache keys

**Type:** bug fix + small feature (typed status) · **Size:** M · **Depends on:** B01
**Unblocks:** F05, F09

## Problem

`BatchExecutor` emits `DefaultHash::default()` (all zero) for completed uncached tasks
(`no_cache`, persistent, cache-disabled). The next task's context forwards that sentinel
as a dependency digest, so a cache-enabled task downstream of an uncached task can be
keyed on a constant that proves nothing about freshness.

## Why it matters

Standalone: stale results can be reused after an uncached dependency changed. For
plugins: inferred tasks are uncached by default, so cache-enabled handwritten tasks that
depend on them hit this immediately. The RFC requires it before plugin work.

## RFC references

- [§8.3 "Cache-enabled tasks and uncached dependencies"](../../008.02-execution-cache-semantic-identity.md#cache-enabled-tasks-and-uncached-dependencies)
- [§7.5 C25](../../007.04-composition-golden-fixtures.md)
- [§13.3 prerequisite note](../../013.00-implementation-plan.md#133-phases)

## Code starting points

- `crates/omni_task_executor/src/batch_executor.rs` (~L655-680: zero hash selection)
- `crates/omni_task_executor/src/task_context_provider.rs` and
  `crates/omni_task_context/src/default_provider.rs` (dependency digest forwarding)
- `crates/omni_task_executor/src/cache_manager.rs`, `result.rs`, `notices.rs`
- `crates/omni_execution_plan` (graph edges; `with` is **not** an execution dependency)

## Work

1. **Rule:** a cache-enabled task is reuse-eligible only if every execution dependency is
   cache-eligible. Otherwise it runs ineligible: no lookup, replay or persistence.
   Propagate along edges (A → B → C). Commandless/skipped aggregate nodes are transparent.
   Persistent dependency counts as uncached. The zero hash never enters a key.
2. **Typed status** on each task result and dry-run output:
   `eligible | disabled{deciding_source?} | persistent | no_command |
   write_only_no_cache | demoted{direct_dependency, root_cause}`. Derived, not persisted,
   not a cache-key input; recomputed on replay. Bounded for deep graphs.
3. **Phase now:** report what current config can supply
   (`disabled`, `persistent`, `no_command`, `write_only_no_cache`, `demoted`).
   `deciding_source` stays `None` and the explicit-contradiction error is **deferred to
   F05**, because today's config cannot distinguish explicit from inherited. The status
   shape must not change when F05 lands.
4. One aggregated informational note per run/plan (not per edge). Details go to dry-run,
   inspection and tracing.
5. `--no-cache` stays write-only but must not leak the zero sentinel into any persisted
   key.
6. No cache storage or remote format change. Changelog entry (pre-v1 correction).

## Out of scope

- Computing real digests for uncached tasks.
- Origin tiers, explicit-contradiction error (F05), CLI/MCP rendering beyond existing
  output channels (F09).

## Acceptance

- C25 scenarios: cached → uncached dependency demotes; chain propagates; aggregate node
  transparent; `with` does not demote; persistent dependency demotes.
- Test asserts no persisted key ever contains the zero hash.
- Unrelated cache-enabled tasks keep hitting.
- Replay recomputes status from the current graph.
- Status visible in task result structs and dry-run output.
