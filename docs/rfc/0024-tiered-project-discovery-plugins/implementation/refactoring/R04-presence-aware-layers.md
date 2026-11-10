# R04: Presence-aware recursive configuration layers

**Type:** refactor (largest, schema/loader) · **Size:** XL (split) · **Depends on:** B02, B03
**Unblocks:** R05, R07, F05, F07

## Problem

`ProjectConfigLoader` eagerly deserializes `ProjectConfiguration`, applying defaults and
rejecting missing values (such as a missing project name) immediately, and the
extension traversal caches flattened processed nodes. Absence of a field is
indistinguishable from an explicit default value, and original list/dictionary
operations are lost.

## Why it matters

Standalone: fixes inheritance surprises (absent vs explicit), enables correct
operator replay across `extends`, and lets defaults apply only at the end. For plugins:
inferred layers must compose beneath explicit ones without importing defaults.

## RFC references

- [§7.2 layer order and merge semantics](../../007.01-layer-order-and-merge-semantics.md)
- [§13.3 phase 1 layer bullets](../../013.00-implementation-plan.md#133-phases)
- [§13.4 loader refactor](../../013.00-implementation-plan.md#134-integration-points-in-current-plumbing)
- [§18.3 project loading and task merge rows](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- Fixtures C02, C06, C07, C13, C14, C17, C18, C22, C23, C26

## Code starting points

- `crates/omni_context/src/project_config_loader.rs` (eager load; resolves `@project`
  cache paths against each file's directory)
- `crates/omni_configurations/src/project_configuration.rs`, `task_configuration.rs`,
  `task_extension.rs`, `cache_configuration.rs`, `task_output_configuration.rs`
- `crates/omni_core/src/extension_graph.rs` (traversal/ordering/cycle checks)
- `crates/omni_context/src/context.rs` (`load_discovered_project_configs`)
- `crates/omni_context/src/project_data_extractor.rs` (applies defaults)

## Slices (separate PRs)

1. **Layer types:** recursive "layer" mirror of project/task/cache/env config where
   every field has presence (`Option`/operation) and keeps original operators. Includes
   cache, key files/env, output, project-to-task settings. Per-side glob presence uses B02.
2. **Layer loader:** parse one file to a layer, keeping per-layer path origin (the
   file's own directory) separate from the effective project root. Replace eager
   deserialization with layer loading; retain the old entry point only as a thin
   "finalize layers" wrapper that uses the corrected semantics.
3. **Replay/finalize:** walk the extension closure in existing order, replay raw
   operations, then apply defaults, resolve `@project` paths against the correct
   root, and validate. Defaults and final validation only after all composition and
   project-to-task inheritance.
4. **Migration fixtures:** old vs new effective config for documented corrections;
   changelog/migration notes.

## Rules to preserve

- Existing extension traversal order and shared-ancestor behavior.
- Name requirement: validated at finalization, not at layer parse.
- Null behavior and per-file origin rules are specified in §7.2: write them down in the
  PR before coding.

## Out of scope

- Plugin layers, `apply`, activation `defaults` (F05), conflict markers (F07),
  alias lowering (F03), virtual identity (R07).

## Acceptance

- Zero-plugin discovery selection unchanged except documented corrections
  (duplicate-file rejection is F01).
- Absence vs explicit-default golden tests for each field family.
- Diamond inheritance, multiple bases, base replacement followed by child append/prepend.
- Operator replay equals old behavior wherever the old behavior was correct.
- Schema/JSON schema output unchanged for valid files.
- Benchmarks (existing discovery benches in `crates/omni_bench`) show no material
  regression.

## Risks

Schema/loader changes touch most config consumers. Land behind small PRs; keep the old
public API compiling through adapters until every caller migrates.
