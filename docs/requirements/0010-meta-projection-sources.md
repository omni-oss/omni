# Requirements: Meta Projection Sources (bundling projections into one source)

- **RFC:** [0010-meta-projection-sources](../rfc/0010-meta-projection-sources.md)
- **Status:** Accepted
- **Date:** 2026-09-03

## Summary

Today every consumer lists each projection source separately in
`workspace.omni.yaml`, and an organization that ships a standard set of
projections must have every consumer paste and hand-maintain that whole list.
This change lets a single projection source bundle several other projection
sources behind one entry. The bundle repository owns the membership, the
per-member pins, and the per-member routes; a consumer adds one source and gets
the whole set, updated by bumping one revision. From the consumer's side nothing
about how sources are declared changes.

## Goals

- Let a projection source ship a bundle manifest that references a nested list of
  projection sources instead of declaring routes for its own files.
- Let each nested source be an ordinary projection source (git or local, with its
  own id, optional inline routes, and/or its own shipped manifest).
- Expand a bundle into its member sources at sync time and run the normal
  fetch, plan, conflict-check, link, and teardown flow over the expanded set.
- Keep an organization's standard set maintainable from one repository, so a
  consumer never edits config when the set grows or a member repository splits.
- Keep a bundle strictly weaker in authority than a hand-written source list: a
  bundle can decide which files get linked, but can never touch control-plane
  settings.
- Keep existing configuration and existing shipped manifests working unchanged.

## Non-goals

- No new field or surface in `workspace.omni.yaml`. The bundling capability lives
  entirely in the source-shipped manifest.
- No unbounded recursion. A bundle may contain bundles, but cyclic or
  pathologically deep bundle graphs are a hard error, never a hang or an
  unbounded fetch.
- No content transformation. Bundled sources are linked or copied verbatim, the
  same as a directly-declared source; bundling only changes which sources
  contribute.
- No bundling for the tool or generator subsystems in this change.

## Requirements

### Configuration

REQ-01 A source-shipped projection manifest must be either a leaf (declares
routes for its own files) or a bundle (declares a list of member sources), and
must not be both or neither.

REQ-02 A bundle manifest's member list must accept the same source definitions as
`workspace.omni.yaml` projections: git or local origin, an id, optional inline
routes, and optionally its own shipped manifest.

REQ-03 Existing leaf manifests must continue to parse with no change to their
content; adopting bundles must not require adding a discriminator or version
field to any existing manifest.

REQ-04 The shape of `workspace.omni.yaml` must not change; a consumer adds a
bundle exactly as they add any other single projection source (an id plus a git
or local origin).

REQ-05 A workspace source whose manifest resolves to a bundle must not also
declare top-level routes; this combination must be a hard error, not silently
ignored.

### Behavior

REQ-06 At sync time the system must expand each bundle into its member sources,
fetching and pinning every member (git or local) through the same mechanism used
for directly-declared sources.

REQ-07 A member source consumed through a bundle must be identified internally by
a system-composed identifier of the form `<bundle-id>::<child-id>`, and this
composed identifier must be used for the ledger, garbage collection, uniqueness,
and teardown.

REQ-08 A member source's authored id must be left unchanged, and the on-disk path
of a namespaced link must be derived from that authored id only; the `::`
composition must never appear in an on-disk path.

REQ-09 A member source consumed through a bundle must land at the same on-disk
location it would if it were declared directly; provenance is distinguished by
the composed identifier, not by path.

REQ-10 The `::` delimiter must be reserved for system composition: an author must
not be able to write it in an id, so a composed identifier can never collide with
a hand-authored one.

REQ-11 The system must reject two members of the same bundle that declare the
same id, with an error that names the bundle and the duplicated child id.

REQ-12 When two bundles contribute namespaced links that resolve to the same
on-disk path, the system must abort the whole sync before writing anything and
report an error that names both bundles and the contested path.

REQ-13 A bundle may contain sources that are themselves bundles; the system must
expand these recursively.

REQ-14 The system must reject a cyclic bundle graph (a source that is its own
ancestor) with an error that names the chain, and must allow a diamond (the same
source reached through two different bundles) as two distinct installations.

REQ-15 The system must enforce a bundle-nesting depth limit that defaults to 16
and can be raised per invocation, and must fail fast when the limit is exceeded
rather than fetch unboundedly.

REQ-16 Routes that arrive through a bundle manifest (inline or from a member's
own shipped manifest, at any depth) must be forbidden from enabling
control-plane settings (`allow_omni_config` / `allow_git`); those remain honored
only from a workspace source's own top-level routes.

REQ-17 A local member source must resolve relative to its parent bundle's root
and must be contained within it; a member path that escapes the bundle root must
be rejected.

REQ-18 Expansion order must be deterministic and a pure function of declared
configuration order, so the set and order of resulting links do not vary between
runs.

REQ-19 Garbage collection (both orphan-link reconciliation and unused-checkout
cleanup) must operate over the fully expanded set, so a full sync never deletes
members it just fetched or reconciles away their links.

REQ-20 `sync --source <arg>` must accept either a whole bundle (its top-level id)
or a single member subtree (a composed identifier), and must fetch only the
selected subtree, not unrelated sources.

REQ-21 A targeted `sync --source` run must never run garbage collection; only a
full, unfiltered sync reconciles removals.

REQ-22 A `sync --source <arg>` that matches no source must be a hard error, not a
silent success.

REQ-23 `unlink <arg>` must tear down either a whole bundle or a single member
subtree using the same matching rule as `sync --source`, operating only on the
ledger, with no configuration read.

REQ-24 `--update` must propagate to bundle members: re-resolving a bundle's
mutable revision re-reads its manifest and re-resolves each member, with parents
resolved before children, and the re-read manifest is authoritative for what is
linked in that run.

REQ-25 When `--update` drops a member that had links on disk, a full sync must
remove those links, and a targeted `sync --update --source <bundle>` must not
remove them but must warn, naming the now-absent member and its lingering links.

REQ-26 The change must not alter the ledger or lockfile format; a fresh clone
must still fully recover its links by running sync.

### Presentation

REQ-27 The human-readable `status` (and `sync`/`--dry-run`) output must render
expanded members as a tree grouped by their composed identifier, one indent per
`::` segment, with each member row showing its local id.

REQ-28 A bundle group header must show a rollup of its members' states as counts
(for example `(2 ok, 1 drifted)`) and must render the bundle name in bold so a
bundle node is visually distinct from a leaf member.

REQ-29 The machine-readable status report must remain a flat list of entries
keyed by the full composed identifier, unaffected by the tree rendering.

### Breaking changes

REQ-30 Previously, `unlink <id>` removed only links whose recorded source id
exactly equalled `<id>`. After this change, `unlink <arg>` also removes every
link whose composed identifier begins with `<arg>::`. Affects: only users of
bundles, since no non-bundle install has a `::` in its identifier; behavior for
existing non-bundle sources is unchanged.

REQ-31 Previously, `sync --source <id>` with an id that matched no configured
source completed silently with success. After this change, an unmatched
`--source` argument is a hard error. Affects: any user (bundle or not) who passes
an unknown or mistyped `--source` value, including scripts that relied on the
silent no-op.

## Migration / upgrade path

No action is required for existing setups. Existing `workspace.omni.yaml` files
and existing shipped leaf manifests keep working with no edits, and there is no
ledger or lockfile migration. The only behavior a user might need to adjust for
is REQ-31: a `sync --source` invocation with an unknown source id now fails
instead of doing nothing, so any automation that passed an invalid id and
depended on a silent success must be corrected.

## Technical Details

- Expansion is a deterministic pre-order traversal of the declared source lists.
  The composed identifier is built by validating each authored id segment and
  then joining the segments with `::`; the composed string is never re-validated
  as an author id, which is what keeps `::` unauthorable.
- Cycle detection is the primary recursion guard and compares source identities
  along the current ancestor path (git origin plus revision, or the canonicalized
  local path). The depth limit is a backstop for the case cycle detection cannot
  catch, such as a repository referencing itself at ever-changing revisions.
- The depth default of 16 is intentionally lower than omni's other nesting limits
  because each level of bundle nesting is a network fetch, so a runaway graph
  should fail sooner.
- `sync --source` and `unlink` share one matching rule: an argument matches a
  source when its composed identifier equals the argument or begins with the
  argument followed by `::`. Targeted selection prunes the traversal so unrelated
  bundles are never fetched.
- The generic parts of the mechanism (traversal, identifier composition, cycle
  and depth guards, selection matching) live in a dedicated crate, generic over
  the member config type and separated from the projection-specific parts (route
  resolution, the privilege floor, the ledger, garbage collection, the namespaced
  path). This lets the traversal be exercised without network or filesystem
  access, and lets a future subsystem reuse it.
- The `status` tree grouping is reconstructed from the composed identifiers in the
  ledger alone (no configuration read), and bundle-name bolding reuses the styling
  already used elsewhere in the CLI, so no new dependency is introduced.

## Out of scope

- Bundling for the tool and generator subsystems. Reusing this pattern there
  would first require those subsystems to gain a shipped-manifest layer they do
  not have today.
- Any content transformation of bundled files.
- Any new field or surface in `workspace.omni.yaml`.
