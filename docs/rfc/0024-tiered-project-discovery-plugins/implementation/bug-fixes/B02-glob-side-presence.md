# B02: Omitted glob include/exclude side decodes as an empty replacement

**Type:** bug fix · **Size:** M · **Depends on:** none · **Unblocks:** R04

## Problem

`MergeGlobConfig` fills in a missing `include` or `exclude` side with
`MergeSingleOrMany::empty()`, which is `List(ListConfig::value(vec![]))`, an **empty
replacing value**. A child that sets only `include` therefore silently wipes the parent's
`exclude` (and vice versa) when layers merge.

## Why it matters

Standalone: a user who writes only `files.include` in a child layer loses inherited
exclusions. For plugins: defaults and plugin baselines are additive layers; losing a
side makes them unsafe. Presence-aware layers (R04) cannot be correct without this.

## RFC references

- [§7.5 C21, C23](../../007.04-composition-golden-fixtures.md)
- [§18.3 MergeGlobConfig row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- [§13.3 phase 1 presence bullet](../../013.00-implementation-plan.md#133-phases)

## Code starting points

- `crates/omni_glob_config/src/lib.rs`: `MergeGlobConfig` (and its `Default`, ~L116).
- `crates/omni_config_types/src/merge_single_or_many.rs`: `MergeSingleOrMany::empty()`
  and its `Default`.
- Consumers: cache key/output file configs; `crates/omni_context/src/project_data_extractor.rs`
  (builds `MergeGlobConfig::IncludeAndExclude` when merging defaults, ~L225).
- RFC 0011 (glob configuration) for intended semantics.

## Work

1. Make each side's presence explicit (e.g. `Option` sides or an equivalent that keeps
   "absent" distinct from "explicit empty"). Keep wire format and JSON schema stable.
2. Merge: replay only the sides that are present; absent side inherits the base.
3. Explicit empty (`include: []` / `replace: []`) still clears.
4. Preserve rule: exclude matching still wins over include.
5. Update all constructors/call sites; keep `normalize()` behavior for final values.
6. Changelog + migration example (zero-plugin behavior correction).

## Out of scope

- The full layer redesign (R04), origin tiers (F05).

## Acceptance

- Include-only child keeps parent's exclude; exclude-only child keeps parent's include.
- Explicit empty list clears that side only.
- Operators (`append`/`prepend`/`replace`/`merge`) per side behave as before when present.
- Round-trip serde and schema parity tests unchanged for valid existing configs.
- C21 and C23 expectations covered by tests (adapt the markdown cases).
