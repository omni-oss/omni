# Requirements: List-valued `match`/`scope` with `!` negation, plus projection field-name cleanup

- **RFC:** [0005-projection-selectors-and-field-names](../rfc/0005-projection-selectors-and-field-names.md)
- **Status:** Accepted
- **Date:** 2026-08-29

## Summary

Projection routing lets authors select source entries with glob fields: `match` on `pattern` and `flatten` rules, and `scope` on `mirror`. Today each is a single glob string, so picking several named paths forces one hard-to-read brace glob, and "mirror everything except a few files" is not expressible. This change lets those fields take either a single glob or a list of globs, where the list is an OR union and a leading `!` marks an exclusion, the same convention omni already uses for task input/output globs. It also cleans up two field names: `explicit`'s `match` becomes `source` (it is a literal path, never a glob), and the `match-kind` key becomes `match_kind` to match the rest of the projection config.

## Goals

- Let `match` (on `pattern`/`flatten`) and `scope` (on `mirror`) accept a single glob string or a list of glob strings.
- Support omni's `!`-prefix negation in those lists so projections behave like task globs.
- Keep the single-string form behaving exactly as before, including existing brace-expansion globs.
- Guarantee an entry selected by several includes still produces exactly one link.
- Update the generated config schema so editors accept both the string and list forms.
- Keep the `match` and `scope` field names distinct because they play different config roles.
- Rename `explicit`'s literal-path field from `match` to `source`.
- Rename the `pattern`/`flatten` `match-kind` key to `match_kind` for a consistent snake_case config surface.

## Non-goals

- Letting `explicit` rules take a list or use `!` negation; the field stays a single literal path.
- Keeping the old `match` (on `explicit`) or `match-kind` keys working; there is no compatibility alias.
- Redefining the `!` negation semantics themselves (parity-based, order-independent, exclude-wins); those are inherited from the shared matcher.
- Per-alternative destinations or per-alternative `match_kind`; all entries in a rule share the rule's `dest` template and `match_kind`.
- Any change to collision handling or the existing-file policy (renamed from `on_collision` to `on_existing` in RFC 0006).

## Requirements

### Configuration

REQ-01 The `match` field on `pattern` and `flatten` rules must accept either a single glob string or a list of glob strings.
REQ-02 The `scope` field on `mirror` must accept either a single glob string, a list of glob strings, or be omitted.
REQ-03 An omitted (or null) `scope` must continue to mean "mirror everything".
REQ-04 The `explicit` rule's literal-path field must be named `source`, and it must remain a single required string (no list, no negation).
REQ-05 The `pattern`/`flatten` per-rule directory-vs-file selector key must be named `match_kind` (snake_case), replacing `match-kind`.
REQ-06 The generated config schema must describe `match` and `scope` as a union of a string and an array of strings so editors accept both forms.
REQ-07 The generated config schema must advertise the field names `source` and `match_kind`; the old `match` (on `explicit`) and `match-kind` names must not appear in it.

### Behavior

REQ-08 A list value must be treated as an OR union: an entry is selected when at least one non-`!` (include) pattern matches it.
REQ-09 A leading `!` on a list entry must mark it as an exclusion, decided by parity: an even number of leading `!` (including zero) is an include, an odd number is an exclude.
REQ-10 Exclusion must always win and must be order-independent: an entry is selected only when at least one include matches and no exclude matches.
REQ-11 An entry matched by several include patterns in the same rule must produce exactly one link, never a duplicate.
REQ-12 A `mirror` `scope` list must select files that any include matches and drop any file an exclude matches; for example `["docs/**", "!docs/drafts/**"]` mirrors everything under `docs/` except the drafts.
REQ-13 The single-string form of `match` and `scope` must parse and behave exactly as before this change, including brace-expansion globs such as `{a,b,c}`.
REQ-14 A `match` or `scope` value that is an empty list must be rejected at config load time.
REQ-15 A `match` or `scope` list that contains an empty or whitespace-only pattern must be rejected at config load time.
REQ-16 A `match` or `scope` list that contains only exclusions (no include) must be rejected at config load time, because it would match nothing.
REQ-17 Invalid glob syntax must continue to surface as an error when the projection plan is computed.
REQ-18 Glob matching must remain deterministic and run on slash-normalized relative paths, so results are identical across platforms.
REQ-19 Projection glob matching must keep its current separator behavior in which `*` and `?` do not cross `/`.

### Breaking changes

REQ-20 Before this change an `explicit` rule used the key `match`; after this change it must use `source`, and a config still using `match` on an `explicit` rule is rejected. Affects: anyone using the `explicit` projection strategy.
REQ-21 Before this change `pattern`/`flatten` rules used the key `match-kind`; after this change they must use `match_kind`, and a config still using `match-kind` is rejected. Affects: anyone setting the directory-vs-file selector on `pattern`/`flatten` rules.

## Migration / upgrade path

Authors using the single-string `match` or `scope` need to do nothing; those keep working unchanged. Authors using the `explicit` strategy must rename each rule's `match:` key to `source:`. Authors setting `match-kind:` on `pattern`/`flatten` rules must rename it to `match_kind:`. There is no alias for the old keys, so an un-migrated config fails to load rather than silently misbehaving.

## Technical Details

The scalar-or-list shape reuses omni's existing `SingleOrMany` type, and matching (both the OR-union and the `!` negation) is delegated to the shared glob matcher introduced in RFC 0007, so there is no bespoke matcher and no separate `exclude` field. Because the matcher returns a single match decision per entry, the "exactly one link per entry" guarantee falls out naturally. The load-time rejections (empty list, empty entry, exclude-only list) are enforced by a validator attached at deserialization, reusing the same parity classifier the matcher uses so the "what counts as an exclude" rule lives in one place. The `match_kind` normalization simply drops a field rename so the key serializes by its snake_case name like the projection config's other keys.

## Out of scope

- Letting `explicit` accept a list or `!` negation.
- Any compatibility alias for the retired `match` / `match-kind` keys.
- Defining or altering the `!` negation semantics (owned by RFC 0007).
- Per-alternative destinations or per-alternative `match_kind`.
- Collision handling and the existing-file policy (tracked separately in RFC 0006, which renames `on_collision` to `on_existing`).
