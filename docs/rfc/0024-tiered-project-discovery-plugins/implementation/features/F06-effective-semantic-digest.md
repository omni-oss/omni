# F06: Per-task effective semantic digest in cache keys

**Type:** feature · **Size:** XL (split) · **Depends on:** R03, R05, B01 (wants F05)

## Problem

The collector hashes raw command config, args, env, input files and dependency digests,
but not the **effective** task after composition/inheritance. Two runs can share a key
while the effective task differs (e.g. inherited settings changed, command context
changed). Raw `Tera`/env expressions can consume changing context that the raw text hash
does not cover. With `cache.key.defaults: false` nothing else would catch definition
changes.

## Goal

Add a named, versioned digest contribution (e.g. `effective-task-config/v1`) computed
once per finalized task from execution-relevant effective fields, mandatory in every key
including when `cache.key.defaults: false`. Not a virtual file input.

## RFC references

- [§8.3](../../008.02-execution-cache-semantic-identity.md#83-fine-grained-execution-cache-integration)
- [§18.3 extraction/collector/project-hasher/cache-manager rows](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)

## Code starting points

- `crates/omni_context/src/project_data_extractor.rs` (builds `CacheInfo`)
- `crates/omni_collector/src/collector.rs` (final digest)
- `crates/omni_context/src/project_hasher.rs` (constructs `ProjectTaskInfo` directly)
- `crates/omni_cache/src/cache/impls/hybrid.rs` (task lookup + stale pruning recompute
  digests)
- `crates/omni_task_executor/src/cache_manager.rs` (dry-run branch builds collector
  inputs directly)
- `crates/omni_hasher` (existing content hasher)

## Slices

1. **Audit doc:** every execution-relevant effective field and runtime context value;
   mark included / excluded (provenance, timings, notices, redaction, display-only) /
   already covered by ordinary inputs. Metadata used in command expressions is not
   display-only. Conservative inclusion where narrowing is unproven.
2. **Digest function:** streaming, fixed-order, type/length-framed encoding over omni's
   hasher (not `std::hash`); canonicalize maps, preserve list order; interpretation mode
   (literal vs expression, R03) included. Version tag. Golden vectors.
3. **Plumb** through `CacheInfo`/task info/collector for **all** constructors:
   executor, dry-run, `ProjectHasher::hash`, hybrid store lookup and staleness pruning.
4. **Cache semantics:** keys change once (expected invalidation at upgrade; document).
   Do not change cache storage or remote formats.
5. Benchmark digest computation.

## Acceptance

- Changed effective semantics miss; unrelated sibling task changes don't change this
  task's digest; presentation-only changes don't; identical semantics keep the same
  digest; explicit overrides mask irrelevant inherited changes; inherited execution
  changes invalidate.
- `cache.key.defaults: false` still invalidates on definition change.
- Project/workspace hash and cache stale-prune agree with executor (parity test).
- Dry-run computes the same digest as real run.
- Golden/encoding stability tests.
