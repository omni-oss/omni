# Requirements: Workspace Projection Enhancements (directory routing + owned projections)

- **RFC:** [0002-projection-enhancements](../0002-projection-enhancements.md)
- **Status:** Accepted
- **Date:** 2026-08-27

## Summary

This change adds two capabilities to workspace projections. First, the `pattern` and `flatten` routing strategies can be told to match and link whole directories, not just files, so a single rule can install each folder under a globbed path as its own link. Second, a source repository can ship its own projection manifest describing how it wants to be projected, so a consuming workspace can inherit that intent instead of restating every routing rule, while still being able to override it. Both build on the projection subsystem and keep the workspace in ultimate control.

## Goals

- Let `pattern`/`flatten` match directory entries and emit one directory link per match, behind an explicit per-rule opt-in.
- Make directory-selecting behavior default-off so every existing file rule is unchanged.
- Define deterministic overlap resolution when directory matches nest.
- Ensure a matched directory from a local source pins on its recursive contents so its content changes are detected.
- Let a source repository declare its own projections in a discovered manifest, so consumers can add the source and sync without restating routes.
- Define precise precedence between a workspace's declared routes and a source's declared routes.
- Prevent a source-declared projection from granting itself elevated filesystem privileges.
- Remove the redundant self-nesting in the projection config by renaming the per-source inner list.

## Non-goals

- No new routing strategy; directory routing is an opt-in on the existing `pattern`/`flatten`.
- No content transformation; projections still link or copy verbatim.
- No per-project projection configuration; owned projections live in the source, not in a consuming workspace's projects.
- No new destination root vocabulary; owned projections still resolve destinations against the consuming workspace.
- No imperative install/add command.

## Requirements

### Configuration

REQ-01 The per-source inner list of projections must be named `routes` (previously `projections`); the top-level `projections` key that lists projection sources is unchanged.

REQ-02 A projection must be identified by a `strategy` value that is one of `mirror`, `explicit`, `pattern`, `flatten`, or `namespaced`.

REQ-03 Every projection must accept the common options `target`, `on_existing` (renamed from `on_collision` in RFC 0006), `link`, `allow_omni_config`, and `allow_git`.

REQ-04 A `mirror` projection must accept a single optional `scope` glob and must not accept a list of scope rules.

REQ-05 An `explicit` projection must accept a `rules` list where each rule has a required `match` and a required `dest`.

REQ-06 A `pattern` projection must accept a `rules` list where each rule has a required `match`, a required `dest`, and an optional `match-kind`.

REQ-07 A `flatten` projection must accept an optional `rules` list where each rule has a required `match`, an optional `dest`, and an optional `match-kind`.

REQ-08 A `namespaced` projection must not accept any `rules`.

REQ-09 The `match-kind` option must accept `file` (the default) or `dir`, and must be available only on `pattern` and `flatten` rules.

REQ-10 The system must reject, at config parse time, any projection that carries fields not valid for its strategy, including `rules` on `namespaced`, a missing `dest` on `explicit`/`pattern`, more than one `mirror` scope, `match-kind` on a strategy other than `pattern`/`flatten`, and any otherwise-unknown key on a projection or rule.

REQ-11 A workspace projection source's `routes` must be optional: absent or null means inherit the source's declared routes, a non-empty list means override, and an empty list is a misconfiguration.

REQ-12 A source repository must be able to ship a projection manifest named `projection.omni` with a `.yaml`, `.yml`, `.json`, or `.toml` extension, located at the source root.

REQ-13 The source-declared projection manifest must carry only a `routes` list and must not carry an `id` or a source definition.

### Behavior

REQ-14 A rule with `match-kind: dir` must match directory entries only, must emit exactly one directory link per matched directory, and must never emit file links.

REQ-15 A rule with `match-kind: file` (the default) must preserve the existing file-matching behavior byte-for-byte.

REQ-16 A single projection must be able to mix file rules and directory rules, treating them as independent rules.

REQ-17 For a directory match, destination template expansion must run against the directory's path components, with `{basename}` being the directory name.

REQ-18 A `flatten` directory rule with no `dest` must default the destination to `{basename}` so a dotted directory name is not truncated.

REQ-19 Within a single projection, the system must drop any candidate link whose destination would fall strictly inside another candidate directory link's destination (shallowest-wins), so nested `**` directory matches resolve deterministically.

REQ-20 (superseded by RFC 0006) Two candidate links that resolve to the identical destination are a plan collision: the sync fails and reports them, regardless of the `on_existing` policy (renamed from `on_collision`). They are never silently pruned. Originally, RFC 0002 resolved them through the configured `on_collision` policy at apply time; RFC 0006 makes an identical-destination collision always fatal.

REQ-21 (superseded by RFC 0006) Overlaps across different projections or different sources are a plan collision and fail the sync, whether the destinations are identical or one nests strictly inside another projected directory link. They are never pruned across the run. REQ-19's shallowest-wins pruning still applies, but only within a single projection; a nested overlap that spans projections or sources is now fatal instead of writing through the outer directory link.

REQ-22 A matched directory from a local source must be content-pinned on the recursive contents of that directory, so adding, removing, or editing a file beneath it changes the pin.

REQ-23 An `explicit` rule naming a directory in a local source must sync successfully (correcting a prior failure in the content-pin path).

REQ-24 When emitting a directory link, the system must run the control-plane and `.git` refusals over the non-symlinked entries beneath the linked directory, refusing unless the corresponding `allow_omni_config`/`allow_git` flag is set.

REQ-25 The system must not discover or load an omni manifest that lives inside a symlinked directory link (only copy-kind directory links expose real files to discovery).

REQ-26 When a workspace source omits `routes`, the system must load the source's projection manifest after materialization and use its `routes`.

REQ-27 When a workspace source provides a non-empty `routes` list, the system must use that list wholesale and ignore any source-declared manifest, replacing rather than merging.

REQ-28 When a workspace `routes` list overrides an existing source manifest, the system must not warn on a normal sync and must emit only a debug-level note naming the shadowed manifest.

REQ-29 A source-declared manifest's destinations must resolve `@workspace`/`@target` against the consuming workspace.

REQ-30 The `allow_omni_config` and `allow_git` escape hatches must be honored only when set in the workspace configuration; a source-declared projection that sets either must be rejected.

REQ-31 A workspace source with an empty `routes` list must fail fast with a sync-time error before the source is materialized.

REQ-32 A workspace source that omits `routes` and whose source ships no (or an empty) manifest must fail with a sync-time error after materialization, rather than silently projecting nothing.

REQ-33 Both no-routes error conditions must be surfaced by a dry-run sync.

REQ-34 A source-declared manifest must be discovered honoring `.omniignore`, as a single manifest at the source root rather than globbed across the tree.

### Breaking changes

REQ-35 Projections are unreleased, so these changes have no external consumers and are pre-release refinements rather than breaking changes; the only in-repository configuration affected is the workspace's own config, updated in the same change.

REQ-36 A `mirror` scope previously spelled as a one-element rules list must now be spelled as a single `scope` glob; the list form no longer parses.

REQ-37 Configurations that were previously accepted but later failed a runtime validator (such as rules on `namespaced`, a missing `dest`, or more than one `mirror` match) must now fail at config parse time instead.

## Migration / upgrade path

No action is required from external users, because projections are unreleased and have no external consumers. Within this repository, the workspace configuration must adopt the renamed `routes` key, the `mirror` `scope` form, and the strategy-tagged shape as part of this change. Existing valid file-mode `pattern`/`flatten` rules require no changes.

## Technical Details

- Directory linking is not a new strategy but a per-rule opt-in on the existing `pattern`/`flatten` strategies, reusing the existing applier, scanner, and ledger; the applier already materializes directory links.
- The projection type becomes a strategy-tagged shape with strongly-typed per-strategy variants and rule types, so relationships previously enforced by runtime validators become invalid states that fail at parse time. Adopting this shape is gated on an implementation-time test confirming that unknown cross-strategy fields are rejected at the pinned serialization-library version; if that does not hold, a thin runtime check is used for the cross-strategy fields instead.
- Owned-manifest discovery reuses the same mechanism generators use to discover their manifest, runs after materialization and before planning, and yields an owned list of routes consumed by the sync engine.
- Directory content pinning reuses the existing scan and hasher to expand a matched directory to its recursive files.
- Directory links created as symlinks or junctions are not reflected in a consuming project's input hash (a known, pre-existing limitation); the interim remedy is to use `link: copy` for content that must participate in a project's cache key. A general fix (having the hash collector follow symlinks, bounded and separate from config discovery) is deferred to a separate RFC and this change does not depend on it.

## Out of scope

- Adding a sixth routing strategy for directories, always-on directory matching, or element-wise merging of owned and workspace routes.
- Content transformation during projection.
- Per-project projection configuration.
- New destination root vocabulary.
- An imperative install/add command.
- Making symlink/junction directory-link contents participate in input hashing (deferred to a separate RFC).
