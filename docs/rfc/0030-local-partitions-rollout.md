# RFC 0030: local cache partitions rollout

> Note: mentions of `omni cache purge` or `--legacy` below are superseded by `0030-operator-tools-rollout.md` (slice 8 was reduced to isolated-entry expiry).

Plan for slice 6 of [RFC 0030](0030-remote-cache-trust-model.md) (section 4.4: local cache
partitions, trust epoch, origin markers, per-task `cache.remote: false`, `--no-remote-cache`). The RFC
stays the design authority. Signing and `verify` are slice 7, so the verify mode is the constant
`off` and the trusted key set is empty here; the epoch is built so slice 7 only supplies values.

## Assessment

Large work, High risk (caching: a wrong hit or a stale partition silently corrupts or poisons
builds). Plan, independent Opus review of the plan, two sub-slices each with its own Opus review
before commit.

- **6a: partitioned local storage** in the hybrid store: partitions, trust epoch, trust class from
  the probe, origin marker with staged extraction, prune and stats across partitions.
- **6b: opt-out**: `cache.remote: false` (config layers, inheritance), `--no-remote-cache`, the
  forward closure of opted-out tasks, routing them to `local/` with remote disabled, and the
  per-item flag through `get_many`/`cache_many`.

## Goal and done

6a:

- With remote caching enabled, every local entry lives under one partition of the cache directory:
  `trusted/{epoch}/` or `untrusted/{epoch}/{principal_id}/{scope}/`. A run reads and writes only
  its own class. An untrusted run (the credential holds `write:artifacts` and not
  `publish:artifacts`, per the probe) reads and writes only its own `untrusted/...`
  partition (it does not read `trusted/{epoch}/`; shared entries reach it through the remote
  cache, where they are verified). Anything else is trusted. Unknown scopes
  (no probe answer) are trusted, the strict case.
- With remote caching disabled the layout is unchanged (the cache directory root). Remote-enabled
  runs never read the root, so entries written before this change, or by a disabled run, cannot
  reach them. This deviates from the RFC's "first run is cold" only in that local-only users keep
  their cache; there is no security difference because the root is unreadable to a remote run.
- Trust epoch: first 16 hex of SHA-256 over a length-prefixed encoding of the sorted effective
  trusted key ids (empty for now), the verify mode (`off` for now), the API base URL and the tenant,
  organization, workspace and environment codes. Any change starts a fresh `trusted/` partition.
- `principal_id` and `scope` in the untrusted path are the server-reported values (from the probe:
  `X-Omni-Principal-Id`, `X-Omni-Scope`), passed through the same filesystem-safe segment rule as the
  server (lowercase literal or `h-` plus 32 hex of SHA-256), never the raw client label. If the
  probe does not report them for an untrusted run, the run is treated as trusted (the strict case,
  decision 4).
- Restores from the remote are written into the run's own partition: an untrusted run writes both
  shared and isolated restores into its `untrusted/...` partition; any other run writes shared
  restores into `trusted/{epoch}/` and never reads `isolated` (the client already reads isolated only
  when it can write; a trusted run that can write isolated is a publisher whose reads of isolated
  entries are refused, see decision 5).
- Origin marker `.omni-origin-remote` (contents: origin `shared` or `isolated`, and `verified_by`,
  `none` until slice 7) is written first in a sibling staging directory and the directory is renamed
  into place, so a crash never leaves a remote entry without its marker and a partial extraction
  never lands in the final directory.
- `prune` and `stats` cover every partition and the legacy root; nothing else in the store reads the
  root when remote is enabled.

6b:

- `cache.remote: false` on a task (and a project-level default, inherited by its tasks and
  overridable, merged like `enabled` through `CacheLayer` and `finalize_composed`, and through task
  `extends`), and `--no-remote-cache` for the run.
- A forward-closure pass over the whole plan before the first batch marks each opted-out task and all
  its transitive dependencies as local-only. Local-only tasks neither read nor write the remote and
  use `local/`; each task runs once per run with the strictest treatment. A normal task that depends
  on a local-only task may consume its outputs and writes its own entry to its own partition; a
  normal task never takes `local/` as a hit for itself.
- The per-item `remote` flag is passed to the store (`get_many`, `cache_many`) and stays out of
  `TaskExecutionInfo` serialization, so no digest changes.

Out of scope: signing, `verify`, `trusted_keys` (slice 7); `omni cache clean` of unreadable
partitions and `--legacy` purge of remote entries (slice 8); documentation beyond the guide (slice 9).

## Decisions

1. **One directory per partition, whole store inside it.** The store keeps `base` and resolves a
   partition directory; the collector, last-used database, lock files, staging and publish all
   operate inside the partition. Nothing crosses partitions.
2. **Resolution is async and may wait for the probe.** The partition for a remote-enabled run needs
   the trust class, so the first local access awaits the probe (10 second timeout, once per run, a
   failure is "unknown" and means trusted). This puts one probe before the first local read, a
   latency change recorded in the release note.
3. **Disabled remote keeps the root layout** (above).
4. **Unknown or unusable means trusted-strict.** No scopes, no probe answer, or an untrusted
   credential without a reported principal or scope: trusted class. An unusable untrusted label
   never makes a path from raw client text.
5. **Isolated reads by a trusted run.** A credential that holds `publish` and `write` is a trusted
   publisher. Its isolated namespace is its own, so a hit there is its own earlier write, written
   into `trusted/` only if it was verified; with no verification yet (slice 7) the run restores
   isolated entries into the partition of its class and marks them `isolated`; a run of the trusted
   class never reuses an `isolated`-marked entry (a miss, then a local success replaces it).
6. **Markers are defence in depth.** Partitions are the boundary; the marker records origin for
   diagnostics and for the opted-out run rule.
7. **Replacement.** A marked entry a run must reject is replaced by a local success
   (`publish_staged_dir(.., replace = true)`), otherwise it would force a rebuild forever.
8. **Directory sharing requirement** (RFC text) goes into the guide: runs of different trust tiers
   must not share a writable cache directory.

## Batches

1. **6a store**: partition resolution and epoch (pure functions with tests), probe-gated resolution,
   staged marker extraction, prune/stats over partitions, class rules for reads and writes. Tests:
   epoch changes with each input and not with order; each partition path is confined (hostile
   principal or scope text); an untrusted run cannot read trusted-only writes of another class nor
   write outside its partition; a trusted run cannot read `untrusted/`; unknown scopes are trusted;
   disabled remote uses the root; legacy root entries are ignored by remote-enabled runs; staged
   extraction leaves no marker-less entry after a failure; prune and stats see every partition.
2. **6b opt-out**: config field and layers, inheritance and extends tests, CLI flag, closure
   computation tests (diamond, chain, unrelated, opted-out dependency of a normal task), routing,
   per-item flag, proof that digests are unchanged.
3. Docs, guide (CI recipe fragment: separate cache directories per trust tier), RFC section 8.

Checks per batch: tests for touched crates, clippy `-D warnings`, fmt, wasm32 worker check. The
`omni-tests` e2e runs in CI.

## Risks

- A path built from server text escaping the cache directory (covered by the segment rule and a
  confinement test).
- Stale partitions on epoch change growing disk (recorded; `cache clean` is slice 8).
- A bug that lets a class read another partition (covered by per-class read/write tests).
- Latency of the probe before the first read.

## Open questions for review

- Is keeping the root layout for disabled remote acceptable?
- Is awaiting the probe before the first local access acceptable, or should local reads start in
  `trusted/` and only fall to `untrusted/` after the probe (unsafe: a read could precede the class)?
- Is the split into 6a and 6b right, and is 6a releasable alone (opt-out missing but partitions in
  place)?
- Decision 5: what should a trusted run do with an `isolated` hit?

## Review outcome (Opus; three blockers, all folded in)

- **B1 collector directory roles.** `Collector::with_output_base` moves only the project output
  directories into the partition. The file-hash index and the input-walk exclusion stay on the whole
  cache directory, so a cache directory inside the workspace never feeds another partition's content
  into a digest (collector test). The digest collectors of `omni_context` use the base directory and
  are unaffected.
- **B2 reserved name.** Partitions live under `.partitions/` (dot-prefixed, so it can never be a
  base58 project directory and stats and prune already skip it): `.partitions/trusted/{epoch}`,
  `.partitions/untrusted/{epoch}/{principal}/{scope}`, `.partitions/local`.
- **B3 locks.** Each partition is a self-contained store (its own prune, last-used and replace
  locks). `stats`, `prune`, `force_prune` and stale-staging reclaim walk every partition through
  one-directory views, so a prune holds the lock of exactly the entries it deletes, and none of them
  needs the probe or the network.
- **S1 isolated reads.** Only an untrusted run (write without publish, with a server-reported
  principal and scope) reads its isolated namespace and only when `target` is isolated. Every other
  run reads shared only; this removes the publisher's isolated reads (release note).
  A trusted run that finds an entry marked `isolated` (or with an unreadable marker) removes it and
  treats it as a miss.
- **S2** One resolution per store (`OnceCell`) awaits the existing 10 second probe; refused, failed
  and scope-less probes are trusted. The background probe is gone.
- **S3** Root layout when remote is disabled stays; remote-enabled runs never read it (test).
- **S4** The epoch normalises the URL (scheme and host lowercased, trailing `/` dropped), is
  versioned and length-prefixed, and keeps an absent environment distinct from an empty one.
- **S5** `segment`, `is_safe_literal` and `is_valid_scope_label` moved to `omni_path_utils`, shared
  by the server and the client, with golden tests; the partition path is confinement-tested with
  hostile text.
- **S6** Restores go through a sibling staging directory, the marker is written before the rename,
  and a rejected archive leaves no entry or staging directory; restore and removal hold the shared
  prune lock. Marker text is one line (`v1 origin=<shared|isolated> verified_by=none`), anything
  else reads as isolated.
- Split: 6a (this change: layout, epoch, class, markers, restore) and 6b (opt-out, closure,
  routing); 6b gets its own review.

## 6b as built

`cache.remote` is an `Option<Replace<bool>>` on `CacheConfiguration` and `CacheLayer`, merged like
`enabled` (so an absent value never drops an inherited `false`; tested through composition and
project-to-task inheritance) and carried in `CacheInfo::cache_remote`. `--no-remote-cache`
(`ExecutionConfig::no_remote_cache`, also in the API request) marks every task. Before the first
batch the pipeline computes the local-only set: opted-out tasks and their transitive
dependencies within the plan (`local_only_closure`), only when remote caching is configured. The
`CacheManager` routes each task by name to one of two stores instead of threading a per-item flag
through the store trait: the normal store, or `HybridTaskExecutionCacheStore::new_local_only`, which
has no remote and keeps entries in `.partitions/local`. Digests are untouched (the flag is not in
`TaskExecutionInfo`). A local-only task with no local store misses rather than falling back to the
remote store. Deviation from the RFC text: the per-item flag is replaced by this two-store routing.

## Status

6a implemented. Opus's pre-commit review (no blockers) added: views hash against the whole cache directory (`base_dir`), a known-untrusted run without a reported principal or scope uses `.partitions/unknown/{epoch}`, and a panicking extraction task is cleaned up. A probe that fails outright is still trusted (the RFC's rule). 6b implemented (see above).
