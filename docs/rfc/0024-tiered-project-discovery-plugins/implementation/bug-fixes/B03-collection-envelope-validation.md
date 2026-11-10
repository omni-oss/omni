# B03: Collection decoding can silently drop operator siblings

**Type:** bug fix · **Size:** M · **Depends on:** none · **Unblocks:** R04

## Problem

`ListConfig` and `DictConfig` accept operator envelopes (`append`, `prepend`, `replace`,
`merge`). Dictionary decoding can discard sibling keys, so a malformed declaration such
as an envelope mixed with ordinary entries can be accepted with part of it ignored.
Operator names are also legal task names in some dictionaries, so envelope detection must
be strict and well defined.

## Why it matters

Standalone: typos and mixed shapes are silently accepted today. For plugins: composition
replays raw operations, so a declaration whose siblings were dropped at decode time can
never be repaired or audited later (see C18).

## RFC references

- [§17.1](../../017.00-review-findings-and-prerequisites.md#171-inferred-collection-operations-versus-conflict-detection)
- [§7.5 C17, C18](../../007.04-composition-golden-fixtures.md)
- [§18.3 List/dictionary row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)

## Code starting points

- `crates/config_utils/src/list_config.rs`, `crates/config_utils/src/dict_config.rs`
- `crates/omni_serde_validators` and `crates/omni_configurations/src/validators.rs`
  (existing validators) _(locate `CollectionConfigValidator` or equivalent)_
- `crates/omni_config_types/src/merge_single_or_many.rs` (scalar-or-list wrapper)

## Work

1. Inventory every field that uses `ListConfig`/`DictConfig` and which operators it
   permits (table in the PR description).
2. Add a bounded, strict envelope check that runs **before** the shared decoder can drop
   siblings: an operator object has exactly the recognized operator keys, or it is a
   plain value/dictionary. Define the rule for dictionaries whose keys may equal operator
   names (operator-named tasks must round-trip).
3. Report malformed mixes with the key path.
4. Keep `ListConfig`/`DictConfig` merge behavior unchanged.
5. Document the validation correction as pre-v1.

## Out of scope

- The additive-only plugin output profile (plugin-specific; later).
- Changing operator semantics.

## Acceptance

- Mixed envelope + sibling entries fail with a precise path.
- A task literally named `append`/`replace` round-trips as a task, not an operator.
- All existing valid fixtures still decode identically.
- Schema output unchanged for valid forms.
- Tests for malformed hidden declarations at leaf and subtree boundaries.
