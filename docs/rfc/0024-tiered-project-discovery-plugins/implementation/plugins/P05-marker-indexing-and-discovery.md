# P05: Marker indexing, candidate grouping and discovery integration

**Size:** L · **Depends on:** R07, F01, P02 · **Unblocks:** P06

## Goal

Extend the existing single ignore-aware walk to collect plugin marker candidates, group
them by canonical root with explicit files, and classify roots (inferred-only, extend,
replace, base template, explicit-only) so the host can hand eligible roots to hooks.

## RFC references

- [§6.2 pipeline and lockfiles](../../006.02-discovery-pipeline-and-lockfiles.md#62-pipeline)
- [§13.4 integration points](../../013.00-implementation-plan.md#134-integration-points-in-current-plumbing)
- [§18.3 discovery row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)

## Code starting points

- `crates/omni_context/src/project_discovery.rs` (the one walker),
  `crates/omni_configuration_discovery` (optionally expose marker sets),
  `crates/omni_context/src/context.rs` (`discover_project_paths`,
  `load_discovered_project_configs`)

## Work

1. Add enabled plugins' static marker names to the same walk; **no per-plugin walkers**.
   No matching markers = no root hook invocation.
2. Marker index: candidates grouped by canonical root with real path identity (R02) and
   preserving the ignore/cache-exclusion behavior.
3. Lockfiles are evidence, never project creators: lockfile-only roots create nothing
   and bypass no selection.
4. Root classification and eligibility (selected/excluded/base/replacement/allowlist)
   with the transition table in §6.1. Respect `projects` include/exclude.
5. Load explicit-only/virtual projects via R07 identity; never feed a logical id to file
   reads or hashing.
6. Zero-plugin parity: identical selection to groundwork baseline (benchmarked).

## Acceptance

- Fixtures: inferred-only, extend, replace, missing name, base template, structural
  markers, shared workspace lock, multiple markers per root, reordered discovery.
- Walk count asserted (one traversal regardless of plugin count).
- Bench: no-plugin and unmatched-marker runs show no regression.
