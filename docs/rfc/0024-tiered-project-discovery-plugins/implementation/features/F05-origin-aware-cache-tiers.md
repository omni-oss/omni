# F05: Origin-aware cache enablement

**Type:** feature · **Size:** L · **Depends on:** R04, B04 · **Unblocks:** F06 (final digest inputs), F09

## Problem

`project_data_extractor.rs` collapses cache enablement with
`cache.enabled.map(..).unwrap_or(true)` (~L244). The source of the value (task-explicit,
project-explicit, inherited, default) is lost, so the explicit-contradiction rule from B04
and the "inferred tasks default to uncached" fallback cannot be expressed.

## Goal

Carry **origin tiers** for `cache.enabled` from presence-aware layers through to
`CacheInfo`, and complete the parts of B04 that were deferred.

Tier order (highest wins): task-level explicit/applied → project-level explicit/applied →
plugin-emitted → activation `defaults` → host fallback. Without plugins only the first
two and the fallback exist; implement the **seams** so the plugin tiers slot in without
reshaping.

## RFC references

- [§7.1 standard host-managed defaults](../../007.00-project-modes-and-apply.md#standard-host-managed-defaults)
- [§8.3](../../008.02-execution-cache-semantic-identity.md#83-fine-grained-execution-cache-integration)
- C01, C06, C19, C22

## Code starting points

- `crates/omni_context/src/project_data_extractor.rs` (enablement + key defaults/files,
  ~L213-250)
- `CacheInfo` and task info types (`omni_cache`, `omni_task_context`)
- Status type from B04 in `omni_task_executor`

## Work

1. Represent enablement as `(value, source)` with an ordered source enum; keep it out of
   cache keys.
2. Fill B04's `disabled{deciding_source}` and `demoted` causes.
3. Add the **explicit-contradiction error**: a cache-enabled task whose executable
   dependency is explicitly disabled by an authored source fails planning, naming both
   tasks and file provenance. Evaluate only on executable edges of the planned graph; the
   validation/inspection path may list all contradictions.
4. Host-fallback hook: an "inferred task" marker (unused until plugins) defaulting to
   `false`; handwritten tasks keep default `true`.
5. `extends`-derived tasks inherit baseline inputs but have no `enabled` fallback of
   their own (C06).

## Out of scope

- The activation-level `defaults` config surface (`tasks.cache.enabled`, `key.files`,
  `key.env`): it is part of plugin activation and lands with the plugin config work.
  Only the tier seam is here.

## Acceptance

- C01 tier-matrix cases that don't need plugins (task vs project explicit, fallback).
- Explicit contradiction produces the error with provenance; implicit one demotes only.
- Status shape unchanged from B04.
- No cache key change (golden hashes identical).
