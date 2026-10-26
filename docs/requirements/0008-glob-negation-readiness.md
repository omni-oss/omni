# Requirements: Ready the glob subsystem for negation

- **RFC:** [0008-glob-negation-readiness](../rfc/0008-glob-negation-readiness.md)
- **Status:** Draft
- **Date:** 2026-08-29

## Summary

omni's glob convention reads a leading `!` as "exclude" and counts repeats for parity, but it has no way to match a file whose name actually starts with `!`. There is an accidental backslash escape that only works on Unix and silently fails on Windows. This change adds a portable `\!` escape so a leading `!` can be matched literally on every platform, and it routes the four subsystems that still build glob sets directly through one shared entry point without changing what any of them match. Turning negation on for any subsystem is deferred to later RFCs.

## Goals

- Give the glob convention a defined, cross-platform way to match a name that begins with a literal `!`.
- Keep the four current glob-building subsystems (collector, project hasher, task filter, cache stats) matching exactly what they match today, with an unchanged workspace hash.
- Document the `!` exclude convention and the `\!` escape on the config fields that honor them, so users learn the behavior from the schema.
- Record which subsystems may adopt negation later and which must not, so later feature work inherits the constraints.

## Non-goals

- Turning negation on for the collector, project hasher, filters, or cache stats. That is deferred to feature RFCs.
- Changing what the migrated subsystems match. Their migration must be byte-identical.
- Changing backslash handling anywhere in a pattern other than a leading `\!`.
- Adding negation to `cache.key.files` or `cache.output.files`. Those stay include-only until RFC 0009.
- Policy matching over rooted paths (`omni_capabilities::matching`).

## Requirements

### Configuration

REQ-01 A glob pattern whose first character is `\` followed by `!` must match a name that begins with a literal `!`, and the leading `!` must not be read as an exclude marker.

REQ-02 The `\!` escape must behave identically on Windows and Unix.

REQ-03 Only the exact two-character sequence `\!` is special; a `\` not immediately followed by `!` must reach the matcher unchanged, and `\\` must not act as an escape for a literal backslash.

REQ-04 A `\!` that appears after the leading marker run (for example `foo\!bar`) must not be treated as an escape; the pattern is unchanged.

REQ-05 The exclude parity rule is unchanged: a pattern with an odd number of counted leading `!` is an exclude, an even count (including zero) is an include, and `\!` inside the leading run counts as a literal `!` rather than a marker.

REQ-06 The config schema descriptions for the negation-aware glob fields (generator `files`, projection match/scope/filter globs, workspace discovery globs) must document the `!` exclude convention and the `\!` escape.

REQ-07 The `cache.key.files` and `cache.output.files` schema descriptions must not document negation; those fields treat a leading `!` as a literal character until RFC 0009.

### Behavior

REQ-08 `\!foo` includes a name beginning with a literal `!` (`!foo`); `!\!foo` excludes that same name.

REQ-09 The collector, project hasher, task filters, and cache stats reporting must match exactly the same files, tasks, projects, and entries after this change as before it.

REQ-10 The workspace hash produced by `omni hash -r workspace` must be unchanged by this work for any workspace that contains no `\!` pattern.

REQ-11 A pattern with no `\!` in its leading marker run must be classified and matched byte-identically on every platform, before and after the change.

### Breaking changes

REQ-12 A pattern whose leading marker run contains `\!` changes meaning. Old behavior: on Unix the leading `!` matched literally by accident, on Windows it was unmatchable. After this change, the leading `!` matches literally and identically on both platforms. Affects: any discovery, generator, or projection config that already relies on a leading `\!`. No such config exists in this repository, and no cache key depends on these subsystems.

## Migration / upgrade path

No action is required from users. Configs that contain no `\!` pattern behave exactly as before, and the workspace hash does not move. A config that already uses a leading `\!` on Unix keeps the same result; the same config on Windows starts working where it was previously broken. The enabling of negation for input/output globs is a separate change (RFC 0009) and carries its own changelog note.

## Technical Details

The escape is resolved in omni's own pre-pass above the underlying glob engine, so the backslash is consumed before the pattern reaches the engine and the result never depends on the engine's platform-dependent backslash handling. The pre-pass is a single left-to-right scan of the pattern start: an unescaped `!` counts one marker, a `\!` pair appends a literal `!` and drops the backslash, and the first other character ends the scan with the rest of the pattern appended verbatim.

The consolidation adds one include-only entry point that shares the existing memoized glob cache, so a set built through it is the same cached object as before. The collector keeps a raw compiled set so it retains its candidate fast path. These are behavior-preserving refactors, verified by a golden-hash snapshot test and an object-identity (Arc-aliasing) test written before the change lands.

## Out of scope

- Negation for any of the four glob-building subsystems (deferred to feature RFCs).
- Negation for `cache.key.files` and `cache.output.files`, including the collector rework needed to support it (RFC 0009).
- Backslash handling elsewhere in a pattern.
- Policy matching over rooted paths.
