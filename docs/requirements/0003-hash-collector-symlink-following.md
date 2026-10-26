# Requirements: Hash-collector symlink following

- **RFC:** [0003-hash-collector-symlink-following](../rfc/0003-hash-collector-symlink-following.md)
- **Status:** Implemented
- **Date:** 2026-08-28

## Summary

Today omni's input hashing skips symlinked directories, so content reachable
through a symlinked directory in a project's input scope never contributes to
that project's input hash. When such a directory is a workspace projection,
changes behind the link do not invalidate the consuming project's cache and the
project serves stale output — a silent cache-correctness bug. This change makes
input hashing follow symlinks so that linked content is hashed, while keeping
that content bounded to the workspace so a cache key can never depend on
anything outside the workspace root.

## Goals

- Content reachable through a symlinked directory in a project's input scope
  participates correctly in that project's input hash.
- Changes behind a projected (symlinked) directory invalidate the consuming
  project's cache, so it no longer serves stale output.
- Cache keys never incorporate content from outside the workspace root.
- The existing anti-injection guarantee for configuration discovery is
  preserved (manifests planted behind a symlinked directory are still never
  loaded).
- The `.gitignore`/`.omniignore` exclude valve continues to work for
  in-workspace content a user does not want hashed.

## Non-goals

- No change to configuration discovery's traversal; manifests behind a symlink
  stay undiscovered by design.
- No new projection configuration; projections consume the corrected hashing
  behavior transparently.
- No general "follow links everywhere" switch; only input hashing follows
  symlinks.

## Requirements

### Behavior

REQ-01 Input hashing must follow symlinks so that the contents of a symlinked
directory in a project's input scope are enumerated and hashed.

REQ-02 A change to a file behind a symlinked directory in a project's input
scope must change that project's input hash.

REQ-03 Adding or removing files behind a symlinked directory in a project's
input scope must change that project's input hash.

REQ-04 The contents reachable through any followed symlink must be excluded from
the hash unless the symlink's resolved (real) target stays within the workspace
root.

REQ-05 A symlink whose resolved target lies outside the workspace root must be
excluded from the hash — this applies uniformly to both symlinked directories
and symlinked files.

REQ-06 When a symlink is excluded because its target escapes the workspace root,
the system must emit an observable warning identifying the link and its target,
so the exclusion is diagnosable rather than silent.

REQ-07 A symlink cycle within a project's input scope must be skipped without
aborting the hash; hashing must complete successfully.

REQ-08 A broken (dangling) symlink within a project's input scope must be
skipped without aborting the hash.

REQ-09 Content excluded via `.gitignore`/`.omniignore` must remain excluded from
the hash even when it would otherwise be reachable through a followed symlink.

REQ-10 Configuration discovery must not follow symlinked directories; a manifest
placed inside a symlinked directory must never be discovered or loaded.

REQ-11 The symlink-following behavior must be deterministic and consistent
across platforms; on Windows, directory junctions must be treated the same as
symlinks for both following and the workspace-root bound.

### Breaking changes

REQ-12 Previously, a symlinked directory in a project's input scope contributed
nothing to the input hash. After this change, its (in-workspace) contents are
hashed, which changes input hashes for affected projects. Affects: any workspace
with a symlinked directory in a project's input scope.

REQ-13 Previously, a symlinked file in a project's input scope was always hashed
via its target content. After this change, a symlinked file whose target
escapes the workspace root is excluded from the hash. Affects: any project that
hashed an out-of-workspace file through a symlink. Symlinked files whose targets
stay inside the workspace are unaffected.

## Migration / upgrade path

No user action is required. omni is pre-release and no current workspace hashes
input content through symlinks, so there is no existing cache to invalidate.
Users should avoid symlinking out-of-workspace content into a project's input
scope; such links are intentionally excluded from the hash and surface a
warning.

## Technical Details

- Following is enabled only for input hashing; configuration discovery keeps
  symlink-following off, which is what preserves the anti-injection guarantee
  (REQ-10). The two have opposite requirements — correctness (follow) versus
  anti-injection (don't follow) — satisfied independently.
- The workspace-root bound (REQ-04, REQ-05) is enforced by resolving a symlink's
  real target and rejecting it if it does not stay within the (resolved)
  workspace root. The check runs only on symlink entries, leaving the
  non-symlink fast path untouched, so the added cost falls only on symlinks
  (orders of magnitude fewer than regular files).
- The bound is evaluated per link at every depth, so a symlink nested inside an
  in-workspace target that itself points outward is still caught at its own
  level.
- The walker does not apply the per-entry containment check to the walk's own
  root paths, and a walk can be rooted directly at a symlinked directory. The
  walk roots are therefore canonicalize-checked separately before walking, and
  an all-escaping root set skips the walk. This keeps the containment invariant
  whole rather than leaving a hole when an input scope points straight at an
  escaping symlinked directory.
- Broken links are already handled gracefully by existing not-found handling
  (REQ-08); only symlink cycles need new skip-with-trace handling (REQ-07).

## Out of scope

- Changing configuration discovery's traversal to follow symlinks (would re-arm
  the configuration-injection vector).
- Any new projection or user-facing configuration to control following.
- A general option to follow symlinks outside of input hashing.
