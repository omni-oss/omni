# Requirements: Source packs, identity, and the source-config profile

- **RFC:** [0014-source-packs-identity-profile](../rfc/0014-source-packs-identity-profile.md)
- **Status:** Implemented
- **Date:** 2026-09-16

## Summary

omni gains packs. A pack is one installable source that bundles generators, tools, and projections and is adopted with a single `packs:` config entry instead of declaring the same source three times. Source-named units (projections and packs) get an author-declared canonical `name` while keeping the consumer-assigned `id` as the operational handle. The underlying source config type is generalized so an identifier's shape can differ by source kind, which readies omni for a future package registry without a later migration.

## Goals

- Let a consumer adopt a bundle of generators, tools, and projections with one config entry.
- Let a pack author publish those three subsystems, plus other packs they build on, as one unit with one identity and one version pin.
- Give projections and packs an author-owned canonical name they can be known by across consumers and, later, a registry.
- Add the canonical name without breaking existing configs or churning existing projection ledgers.
- Support packs that compose other packs, with the same cycle detection and depth limits omni already applies to projection bundles.
- Introduce no duplicate fetching, no new lockfile, and no new garbage-collection path.

## Non-goals

- Designing the package registry itself (name-to-pin resolution, registry fetch, registry cleanup). This change only makes the types registry-ready and ships an inert stub.
- Adding a fourth runtime subsystem. A pack is an aggregation of the three existing subsystems, not a new build graph, hasher, or executor.
- Cross-pack dependency resolution or semver solving. Every pack is pinned by revision.
- Allowing a pack to carry workspace-global or ambient configuration (environment, capabilities, experimental toggles, ignore rules, UI, or projects).
- Namespacing generator or tool item names in a plain (non-pack) source, or changing routing strategies, trust floors, or hashing.

## Requirements

### Configuration

REQ-01 The workspace configuration gains an optional top-level `packs` list.

REQ-02 Each `packs` entry is a source of kind `git` or `local` and must carry a consumer-assigned `id` on those kinds.

REQ-03 Each `packs` entry may carry an optional `provides` list naming a subset of `{generators, tools, projections}`.

REQ-04 Each pack source must contain a `pack.omni.{yaml,yml,json,toml}` manifest at its root, and only at its root.

REQ-05 A pack manifest has a required `name`, optional `version` and `description`, and optional `generators`, `tools`, `projections`, and `packs` lists.

REQ-06 A pack manifest `name` must be a scoped identifier of the form `@org/name` (or a bare `name`) and is validated against the shared source-name rule.

REQ-07 A pack manifest rejects unknown fields, including any ambient or workspace-global field (`env`, `capabilities`, `enable_experimental`, `ignore`, `ui`, `projects`), as a parse error.

REQ-08 A projection manifest gains optional `name`, `version`, and `description` fields, and a manifest without them continues to parse unchanged.

REQ-09 A `registry` source kind exists in the type but is hidden from serialized output and the published schema, and is never constructed.

### Behavior

REQ-10 A single `packs` entry registers its source into the generator, tool, and projection subsystems at once.

REQ-11 A pack contributes to a subsystem only if its manifest declares the matching source list.

REQ-12 Generator and tool item names contributed by a pack are namespaced by the pack's qualified id (for example `agent-kit::house-style:component`) so they cannot collide with the workspace's own names or another pack's.

REQ-13 A consumer's `provides` narrows the subsystems a pack contributes and cascades to that pack's sub-packs, and it can never widen what a pack offers.

REQ-14 Packs may compose other packs recursively, subject to the same cycle detection and depth cap used for projection bundles.

REQ-15 A single pack manifest may declare its own subsystem sources and compose sub-packs at the same time.

REQ-16 A pack graph is fetched and materialized once regardless of how many subsystems reference it.

REQ-17 A pack manifest `version` is metadata only and is neither a version constraint nor a resolution key.

REQ-18 Pack source paths are resolved relative to the pack root, and paths that escape the checkout (`..` or absolute) are rejected.

REQ-19 A `packs` entry whose materialized root has no `pack.omni.*` manifest is a hard error, reported early.

REQ-20 A pack manifest with no `name`, or with all four source lists empty, is rejected.

REQ-21 The projection ledger keys links by the consumer-assigned `id` (and the qualified id composed from it), never by the author `name`, so adding or changing a `name` re-keys nothing.

REQ-22 omni gains read-only inspection commands `omni pack list`, `omni pack tree`, and `omni pack info <qualified-id>`.

REQ-23 `omni remote-sources install` materializes packs through the existing shared source store; no separate pack install command is added.

REQ-24 A `packs` list rejects duplicate git URIs and duplicate `id`s within the list.

REQ-25 The author canonical `name` is the display label shown in `omni pack` and `omni projection status`.

### Breaking changes

REQ-26 This change introduces no breaking changes to existing workspace configs: all new fields are optional, the existing `generators`, `tools`, and `projections` source lists are unaffected, and the on-the-wire shape of every existing source is preserved.

## Migration / upgrade path

No action is required from existing users. Existing workspace configs, projection manifests, and source lists parse and behave exactly as before. Authors who want the new capabilities opt in by adding a `packs` list (consumers) or a `pack.omni.*` manifest and canonical `name` (authors). One caveat for pack authors. Because a pack's `id` composes the qualified id that keys its contributed projections, renaming the `id` re-keys those projections; the author `name` is deliberately not this key, so renaming the `name` does not.

## Technical details

- The source config type is generalized from a single flattened `extra` per variant to a per-variant profile with a shared base slot, mirroring the existing input-profile pattern. This is the foundational change and lands first; the identity model and packs build on it.
- The consumer-assigned `id` is required on `git`/`local` because a source's own name lives inside a checkout that has not been fetched yet, while the pre-fetch traversal that composes qualified ids and prunes unselected subtrees runs before fetch. Under a future registry the name is known pre-fetch, so `id` becomes optional and defaults to the name.
- Pack expansion reuses the existing meta-source traversal (cycle detection, depth cap, qualified-id composition, duplicate-sibling rejection, subtree selection). Expansion happens at a use-time point, not during config load, so config load stays offline and parse-time validators still see only the authored lists.
- Expanded pack sources feed the three existing effective-source read sites; the shared content store dedups by commit, and refs are recorded under whichever subsystem consumes each contribution, so no new lockfile or GC namespace is needed.
- The pack manifest deserializes through a private, strict intermediate representation so the no-ambient-fields rule is enforced as a parse error rather than by convention.

## Out of scope

- The package registry (name-to-pin resolution, fetch transport, registry cleanup); only an inert, documented stub ships now.
- Invoking a pack-contributed generator or tool by its unqualified name when unambiguous.
- Letting a consumer reroute or rename a pack's projections without forking.
- Letting a projection manifest both declare routes and compose sub-sources in one node (projections keep their either-routes-or-sources constraint).
- A per-pack recommended capability floor that a consumer opts into.
