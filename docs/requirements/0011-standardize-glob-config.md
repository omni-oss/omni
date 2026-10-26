# Requirements: Standardize glob configuration on a structured include/exclude type

- **RFC:** [0011-standardize-glob-config](../rfc/0011-standardize-glob-config.md)
- **Status:** Accepted
- **Date:** 2026-09-03

## Summary

Every glob field in omni config is a flat list of patterns today. A flat list reads like a `.gitignore`, so authors expect a leading `!` to exclude and expect a later entry to re-include what an earlier one dropped. omni does neither, and the same `!` means three different things depending on which field you are editing. This change replaces the flat list with one structured type across every glob field. A field is a bare pattern, a list of patterns, or an object with explicit `include` and `exclude` lists. Exclusion is expressed only by structure, `exclude` always wins, and match order does not matter. The leading-`!` convention and the `\!` escape are removed. This is a deliberate breaking config change, taken while omni is pre-release.

## Goals

- Give every glob config field one shape and one meaning: cache input/output files, workspace `projects` discovery, generator `files`, and projection `match`/`scope`.
- Express exclusion only through an explicit `include`/`exclude` object. The bare pattern and list forms are include-only.
- Make exclusion unordered and exclude-wins, matching what omni already computes and what Bazel, Rollup, and tsconfig do.
- Remove the leading-`!` convention and the `\!` escape from every config field.
- Keep resolved cache input/output paths byte-identical for any config that is include-only after migration, so no cache key moves.
- Document the new forms on every affected field's schema description.

## Non-goals

- Ordered, gitignore-style re-inclusion where a later pattern re-adds files an earlier one dropped. omni is unordered and stays that way.
- Runtime CLI/MCP selection filters (project/task/dir filters and `get_stats` globs). Those take patterns from the command line, not config, and are out of scope here.
- Policy matching over rooted paths.

## Requirements

### Configuration

REQ-01 Every glob config field must accept three forms: a single pattern, a list of patterns, or an object with `include` and `exclude` keys.

REQ-02 The single-pattern form and the list form must be include-only with an empty exclude set.

REQ-03 A field must express exclusion only through the object form's `exclude` key; a leading `!` in any pattern must be treated as an ordinary literal character.

REQ-04 Cache input and output files (`cache.key.files`, `cache.output.files`) must continue to layer a project's entries onto workspace defaults, keeping the existing `append`, `prepend`, `replace`, and `merge` behavior on each of the `include` and `exclude` lists.

REQ-05 Workspace `projects` discovery, generator `files`, and projection `match`/`scope` must merge last-writer-wins, matching how those fields merge today.

REQ-06 The schema description for every migrated field must document the three forms and the exclude-wins rule, and must not mention the `!`, `!!`, or `\!` conventions.

### Behavior

REQ-07 A file is selected when it matches at least one `include` pattern and no `exclude` pattern.

REQ-08 Exclusion must be unordered: adding a pattern to `include` can never re-include a file that an `exclude` pattern matches. `exclude` always wins.

REQ-09 A leading `!` must match a file whose name literally begins with `!`, on every platform, with no escape.

REQ-10 An include-only config must resolve to the same set of files after migration as it did before, so the workspace content hash does not move for any config that used no `!` negation.

REQ-11 For cache input and output files, the resolved paths must be byte-identical after migration for any config that is include-only.

REQ-12 The set of files that counts a changed file as affecting a task (the SCM-affected filter) must always match the set the cache hashes; the two must never diverge.

REQ-13 An `exclude` pattern must never cause a directory to be walked during collection; only `include` patterns seed the file walk.

REQ-14 A config object with an unknown key must be rejected.

### Breaking changes

REQ-15 Discovery, generator, and projection fields previously read a leading `!` as an exclude marker, counted `!!` for parity, and honored a `\!` escape. After this change, a leading `!` is a literal pattern character and excludes nothing. Affects: any workspace `projects`, generator `files`, or projection `match`/`scope` config that used `!` to exclude. Such configs must move the excluded patterns into an `exclude` list.

REQ-16 The flat-list form can no longer negate on any field. Affects: any author who expected a later list entry to re-include files dropped by an earlier `!` entry; that pattern never worked in omni and now parses as literals.

## Migration / upgrade path

A config that only ever included files needs no change. `files: ["src/**"]` and `files: "src/**"` keep working and resolve to the same set, so cache keys do not move.

A config that used `!` to exclude must be rewritten into the object form. Replace a list like `["src/**", "!src/gen/**"]` with:

```yaml
include: ["src/**"]
exclude: ["src/gen/**"]
```

The migration is mechanical: move every `!`-prefixed pattern into `exclude` and drop the `!`. A changelog note and the updated schema descriptions point authors to the new form. No in-repo config currently relies on `!` negation, and no cache key depends on the negation-aware fields.

## Technical Details

Both config types normalize, after merge, into one owned include/exclude pair that consumers compile. Merge runs on the config representation first, so a project's `append`/`prepend` still layers onto a workspace default before normalization. The two config types differ only in merge behavior: one is last-writer-wins, the other carries the existing list-layering forms per side, so each field keeps the merge rule it has today and the type swap changes no layering.

Path-like patterns normalize their separators to `/` before compilation. Rooted `@workspace/`-style resolution stays with the consumer that holds the root map, not in the pattern type, because the root map is a runtime value not known at parse time.

The include/exclude matcher and the compiled glob sets already exist from prior work; this change removes the leading-`!` parity scan and the `\!` escape that fed them and hands the matcher two explicit lists instead. The include-only migration is guarded by an existing golden-hash snapshot test.

## Out of scope

- Ordered, gitignore-style re-inclusion.
- Runtime CLI/MCP selection filters (project/task/dir filters and `get_stats` globs).
- Policy matching over rooted paths.
