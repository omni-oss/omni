# B01: Cache lookup and replay ignore `cache.enabled: false`

**Type:** bug fix · **Size:** S · **Depends on:** none · **Unblocks:** B04, F06

## Problem

`cache.enabled: false` stops entries being **written** but not **read**. A pre-existing
local or remote entry with unchanged command/inputs is still restored after a task (or
a workspace/plugin default) disables caching. Persistent tasks have the same gap.

## Why it matters

Standalone: disabling caching must disable it. For plugins: inferred tasks default to
uncached, and the opt-in policy is only trustworthy if lookup respects it. The RFC makes
this a precondition for any plugin work.

## RFC references

- [§8.3](../../008.02-execution-cache-semantic-identity.md#83-fine-grained-execution-cache-integration) (concrete correction paragraph)
- [§7.5 C24](../../007.04-composition-golden-fixtures.md)
- [§13.3 phase 2 prerequisite note](../../013.00-implementation-plan.md#133-phases)

## Code starting points

- `crates/omni_task_executor/src/cache_manager.rs`: `CacheManager::get_cached_results`
  collects every `execution_info()` and calls `store.get_many`. Write path
  (`cache_results`) already checks `cache_info.cache_enabled` and `!persistent`.
- `crates/omni_task_executor/src/batch_executor.rs` (~L660): the write side checks
  `cache_enabled`; replay of returned hits does not.
- `crates/omni_cache/src/exts.rs`: `TaskExecutionInfoExt::execution_info` returns info
  regardless of enablement. **Do not remove it globally**: project hashing and cache
  maintenance consume it.

## Work

1. Filter at the lookup boundary in `get_cached_results`: skip tasks that are
   cache-disabled or persistent before `get_many` (local and remote).
2. Re-check eligibility at replay in `BatchExecutor` so a hit for a now-ineligible task
   is never restored.
3. Keep `--no-cache` (write-only) and force options semantics unchanged.
4. Changelog entry: lookup now honors `cache.enabled: false` (pre-v1 correction).

## Out of scope

- Dependency-based demotion and the typed status (B04).
- Origin-aware `enabled` tiers (F05). Use today's resolved boolean.

## Acceptance

- Test: pre-seeded local entry, unchanged command/inputs, then `cache.enabled: false`:
  the task executes; nothing restored; nothing persisted.
- Same with a remote-cache fixture/mock.
- Persistent task never looked up.
- Unrelated cache-enabled tasks in the same run still hit their entries.
- `--no-cache` and force behavior unchanged (existing tests pass).
- Project hash and cache stats/prune results unchanged for disabled tasks.
