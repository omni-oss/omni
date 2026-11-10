# F08: Remote-source acquisition leases and atomic reference updates

**Type:** feature (shared infrastructure) · **Size:** L · **Depends on:** none

## Problem

`RemoteSourceManager` publishes checkouts and merges pins, but directly replaces each
subsystem's reference document and has no notion of an **active user**. Garbage
collection can reclaim a checkout while a process still reads it, and partial reference
updates can drop another consumer's GC roots.

## Goal

1. **Active-use leases** scoped to an immutable checkout/artifact, held while descriptor/
   module loading or a runtime may read it; released by process exit (crash-safe, no
   stale marker files retaining content).
2. **Coordinated acquisition:** materialize/publish, take lease and GC without an
   unprotected gap.
3. **Atomic reference updates:** complete reconciliation vs additive merge; a failed
   install never replaces a complete set with a partial one.

This benefits generators, tools, projections and packs today.

## RFC references

- [§15.1-15.3](../../015.00-remote-source-integration.md#152-contributor-install-selection-and-references)
- [§17.6](../../017.00-review-findings-and-prerequisites.md#176-artifact-digests-versus-mutable-artifact-loading)
- [§18.3 source manager row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)

## Code starting points

- `crates/omni_remote_source/src/manager/default_impl.rs`
- `crates/omni_remote_source_contributors` (existing contributors, reference writes)
- `crates/omni_api` remote-source operations

## Slices

1. Protocol doc: lock hierarchy, lease file/lock format, GC interaction, platform notes
   (Windows lock semantics).
2. Lease primitive (process-held lock or equivalent) + tests for process death.
3. GC skips leased content, reclaims unrelated artifacts.
4. Reference API: `replace_complete` vs `merge_additive`, atomic write, subsystem
   isolation.
5. Migrate existing contributors to the new reference API.

## Out of scope

- Plugin contributor, bounded artifact-byte capture (the capture part is plugin/runtime
  facing; see F10 for the generic read side).

## Acceptance

- Concurrent acquire/update/GC tests; kill-during-lease test; failed publication leaves
  old references intact.
- Existing remote-source/pack/generator tests unchanged.
- Unselected subsystem references never discarded.
