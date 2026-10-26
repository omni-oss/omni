# Requirements: Prefetching remote sources (`omni remote-sources install`)

- **RFC:** [0013-remote-sources-install](../rfc/0013-remote-sources-install.md)
- **Status:** Accepted
- **Date:** 2026-09-13

## Summary

omni gains a command, `omni remote-sources install`, that fetches and pins every
remote source declared in a workspace ahead of time, for offline preparation, CI
warm-up, and predictable first-run latency. All subsystems (generators, tools,
projections) share one content-addressed source store and one committed pin
lockfile, so a repository used by more than one subsystem is fetched once and
every subsystem resolves it the same way. The store is a prerequisite for the
command, not a later follow-up.

## Goals

- Provide a single command that prefetches all remote sources across every
  subsystem without touching the working tree.
- Fetch each `(uri, commit)` once and share it, removing duplicate clones and
  duplicate pins within a workspace.
- Give every subsystem one shared way to materialize sources, so fetch and pin
  behavior cannot drift between them.
- Keep resolved commits stable, so downstream hashes never move as a result of
  this change.
- Leave room for non-git source kinds without building them now.

## Non-goals

- A machine-global cache shared across workspaces. The store is per-workspace;
  cross-workspace deduplication is future work.
- Any non-git source kind (HTTP, OCI, mercurial, S3). Only git is implemented.
- Folding the projection placement ledger (`links.json`) into the store or the
  pin lockfile.

## Requirements

### Configuration

REQ-01 The system must provide a command `omni remote-sources install` that fetches and pins every remote source declared anywhere in the workspace.

REQ-02 install must accept `--generators`, `--tools`, and `--projections` flags that select which subsystems to process, and must process all subsystems when no such flag is given.

REQ-03 install must accept an `--update` flag that advances mutable refs by re-resolving branches or tags and re-pinning them, with the same meaning as `projection sync --update`.

REQ-04 The workspace must keep a single committed pin lockfile at `.omni/sources/lock.json`, replacing the previous per-subsystem lockfiles.

REQ-05 The pin lockfile must be serialized with sorted keys so its committed diffs are stable regardless of the order in which pins were added.

REQ-06 Remote git checkouts must live in a shared, content-addressed store at `.omni/sources/store/git/<slug>/<commit>/`, keyed by the resolved commit.

REQ-07 Everything under `.omni/sources/` must be git-ignored except the single committed `.omni/sources/lock.json`.

REQ-08 The store root must be a relocatable parameter that defaults to the per-workspace `.omni/sources/store/`.

### Behavior

REQ-09 A given `(uri, commit)` must be materialized once and shared by every subsystem that references it, deduplicating both the on-disk checkout and the lockfile pin.

REQ-10 Every subsystem and `install` must materialize remote sources through one shared path, so fetch, pin, and deduplication behavior is identical across them.

REQ-11 Deduplication must apply on every lazy run (for example `omni gen` and `projection sync`), not only under `install`.

REQ-12 Git tool sources must be fetched, where previously they were never materialized.

REQ-13 Resolved commits must be byte-identical before and after this change and between `install` and the lazy paths; only `--update` may change a resolved commit.

REQ-14 Only `omni remote-sources install` may perform garbage collection, removing store checkouts and lockfile pins that no subsystem references.

REQ-15 Lazy runs must only add to the store and refresh their own reference set, and must never delete shared content.

REQ-16 Two revisions that resolve to the same commit must share one checkout, and advancing a mutable ref must produce a new commit directory rather than mutating one in place.

REQ-17 The store must be disposable: with the committed lockfile present, any run re-materializes every source at its pinned commit, so deleting the store or the reference files loses nothing.

REQ-18 A source whose commit is already locked must materialize offline, without contacting the network.

REQ-19 install must report one summary line per subsystem, covering the counts of materialized, deduplicated, and garbage-collected sources.

REQ-20 Concurrent omni processes must not lose a just-added lockfile pin; a workspace-local advisory lock guards the lockfile read-modify-write.

### Breaking changes

REQ-21 Per-subsystem lockfiles under `.omni/sources/<id>/lock.json` are replaced by one `.omni/sources/lock.json`. Affects: any workspace with existing source lockfiles.

REQ-22 Checkouts move from the per-subsystem, rev-keyed `.omni/sources/<id>/git/<slug>/<rev>/` to the shared, commit-keyed `.omni/sources/store/git/<slug>/<commit>/`. Affects: on-disk source caches.

REQ-23 Lazy runs no longer garbage-collect their own source directories; reclamation happens only through `install`. Affects: anyone relying on per-run cleanup.

REQ-24 The managed ignore block changes from the per-subsystem form (`/.omni/sources/*/**` with `!/.omni/sources/*/lock.json`) to `/.omni/sources/**` with `!/.omni/sources/lock.json`. Affects: the generated ignore file.

## Migration / upgrade path

omni is pre-release, so there is no migration story and no compatibility shims.
The clean layout is adopted directly. Users need take no action: the store and
reference files are derived caches that regenerate on the next run, and the pin
lockfile simply moves to `.omni/sources/lock.json`. The one behavior users
should know about is that reclaiming stale checkouts is now an explicit step
(`omni remote-sources install`) rather than something lazy commands do on every
run.

## Technical Details

- The store is content-addressed and keyed by resolved commit. Materialization
  clones into a temporary directory under the store root, reads the resolved
  commit, then atomically renames the directory into place; a lost race (the
  target already exists) simply discards the temporary copy.
- The temporary directory lives under the store root so the atomic rename stays
  on one filesystem even if the store root is later relocated.
- Garbage collection unions per-subsystem reference sets recorded at
  `.omni/sources/refs/<id>.json`, then deletes any store checkout and lockfile
  pin outside that union. `install` re-runs every subsystem first so the union
  is complete.
- The cross-process advisory lock (`.omni/sources/lock.json.lock`) wraps only
  the lockfile read-modify-write, not the network fetch, to keep its held time
  small.
- The design defines an engine-neutral source abstraction generalizing to
  `store/<kind>/<key>/`, where the key is always an immutable post-fetch content
  identifier and each kind owns its sub-layout. Git is the only implemented
  kind.
- Subsystem contributors are thin and receive already-loaded configuration;
  subsystem discovery stays with the caller that drives them.

## Out of scope

- A machine-global or cross-workspace content store and its cross-workspace
  garbage collection.
- Any non-git source kind; only the abstraction and the git implementation
  exist.
- Folding the projection `links.json` ledger into the store or the lockfile.
- A `--prune` option on lazy runs or a separate garbage-collection verb;
  reclamation stays confined to `install`.
- Fully merging the two duplicated generator-discovery code paths into one
  function; they are routed through the shared path so they cannot drift, but
  collapsing them is deferred.
- Concurrency coordination beyond the workspace-local advisory lock on lockfile
  writes.
