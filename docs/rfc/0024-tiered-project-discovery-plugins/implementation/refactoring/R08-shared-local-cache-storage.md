# R08: Dependency-neutral local cache storage primitives

**Type:** refactor · **Size:** L · **Depends on:** none

## Goal

Extract the local on-disk storage mechanics from the task cache (atomic writes,
integrity envelope, bounded lookup, pruning, concurrency/crash handling) into a layer
that does not know about task execution metadata, archives or planning. Migrate the
task cache onto it with **no change to task cache formats or remote protocol**.

## Why it matters

Standalone: smaller, testable storage layer; clearer failure handling. For plugins: the
inference cache is a second consumer and must not couple to task types or fork storage
code.

## RFC references

- [§8.2](../../008.01-inference-cache-and-storage.md#82-shared-storage-separate-task-and-inference-caches)
- [§18.3 task cache interface row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- [§13.2 `omni_cache` row](../../013.00-implementation-plan.md#132-existing-crates-to-change)

## Code starting points

- `crates/omni_cache/src/cache/traits.rs`, `crates/omni_cache/src/cache/impls/hybrid.rs`
- `crates/omni_remote_cache_*` (must remain untouched in format)

## Slices

1. Read-only audit: which parts of `hybrid.rs` are generic storage vs task semantics.
2. Define the neutral local API (keyed records, integrity check, bounded reads,
   best-effort vs required write policy as a caller choice, prune hooks).
3. Move task storage onto it; keep every failure/transport behavior.
4. Do **not** add the inference cache here; just ensure a second consumer is possible
   (a small test consumer proves the API is generic).

## Acceptance

- Existing cache tests pass unchanged; on-disk task layout byte-compatible with
  existing entries (test reading entries produced by the previous version).
- Corruption, concurrent prune and crash-recovery tests at the storage layer.
- Test consumer with a different key/record type works without task dependencies.
- No remote-cache code or format changes.
