# R07: Virtual project identity and raw layer traversal

**Type:** refactor · **Size:** L · **Depends on:** R04 (and R02 for ids)
**Unblocks:** F07

## Problem

Context loading discards the `Virtual` classification and eagerly processes extension
nodes into flattened results. There is no way to represent a project that has **no
physical config file** (a stable logical key) and no way to read back the ordered raw
layers of an explicit extension closure without importing root-specific effective
values.

## Why it matters

Standalone: an explicit-optional project, inspection of where a setting came from, and
cleaner `extends` (extending a file never imports that root's computed defaults). For
plugins: inferred-only projects have no file; extending an inferred project's file must
not import its inference.

## RFC references

- [§13.5 risky changes (virtual identity)](../../013.00-implementation-plan.md#135-risky-changes-requiring-care)
- [§13.3 phase 1 virtual identity and raw layers bullets](../../013.00-implementation-plan.md#133-phases)
- [§18.3 discovery row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- C09, C13

## Code starting points

- `crates/omni_context/src/project_discovery.rs` (classification),
  `project_config_entry.rs`, `context.rs`
- `crates/omni_core/src/extension_graph.rs` (reuse the graph's associated identity type;
  the trait itself need not change)
- `crates/omni_types` (logical key / provenance types, if identity changes)
- Hash consumers: `project_hasher.rs`, `workspace_hasher.rs`, `omni_collector`

## Work

1. Add a synthetic but stable **logical project key**, kept separate from optional
   physical config file locations. `dir` remains the real project root, never a
   synthetic directory.
2. Never pass a logical id into file-read or hash-input APIs. Add a type-level or test
   guard.
3. Update discovery classification and loader to carry real-or-virtual configuration
   location; inspection APIs round-trip it.
4. Expose **ordered raw explicit layers** from graph traversal retaining merge
   operations, excluding any root-specific effective/inferred values.
5. Adding/removing an explicit override preserves root identity and status without
   manufacturing a physical file.

## Out of scope

- Any inference source of virtual projects (needs plugins). Provide only the capability
  and unit tests with synthetic fixtures.

## Acceptance

- Test-only virtual projects are runnable and selectable by name/root.
- No logical id ever reaches file reads or hashing (tests assert).
- Raw-layer traversal reproduces closure order incl. diamonds; no effective values leak.
- No change to existing task-cache formats or keys.
