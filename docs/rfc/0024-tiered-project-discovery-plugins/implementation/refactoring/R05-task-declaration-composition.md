# R05: Field-wise task composition and atomic alias transitions

**Type:** refactor + intentional pre-v1 correction · **Size:** M · **Depends on:** R04
**Unblocks:** F03, F06

## Problem

`TaskConfiguration::merge` field-merges long/long but **replaces the whole task** for
short/long, long/short and short/short pairs. `into_long_form` also injects defaults and
an implicit upstream self-dependency, so reusing it for partial overrides invents values.

## Why it matters

Standalone: `build: cargo build` in a base plus a long-form override should not erase
fields unexpectedly. For plugins: users override inferred tasks with partial patches.

## RFC references

- [§17.9](../../017.00-review-findings-and-prerequisites.md#179-task-declaration-kind-composition)
- [§7.2](../../007.01-layer-order-and-merge-semantics.md), [§7.4](../../007.03-task-scoping-aliases-and-selectors.md)
- Fixtures C07, C08, C10, C11

## Code starting points

- `crates/omni_configurations/src/task_configuration.rs`
- `crates/omni_configurations/src/task_extension.rs`
- `crates/omni_core/src/extension_graph.rs`

## Work

1. Normal-task pairs (short/short, short/long, long/short, long/long) merge field by
   field with the same per-field rules. Expand shorthand **once** into command +
   implicit upstream dependency; every other field absent (no eager defaults).
2. Normal ↔ alias transitions replace the entry atomically; nothing survives
   normal→alias; alias→normal does not import target settings; alias/alias takes the
   higher explicit target without merging.
3. Preserve operation order, declaration-local template status, original reference
   binding. Input validation still rejects an alias carrying task overrides.
4. Commandless tasks stay valid.
5. Changelog + old/new examples.

## Out of scope

- Qualified `plugin` references (F03). Alias-aware selection (F04).

## Acceptance

- All four shorthand/long pairs: retained command/env/cache fields, implicit
  dependency add/remove, dictionary replacement, `extends` layers, cycles, missing
  targets, malformed mixed declarations.
- Alias/alias replacement; normal↔alias atomicity.
- Migration examples show before/after.
