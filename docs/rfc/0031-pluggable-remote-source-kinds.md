# RFC 0031: Pluggable remote-source kinds

- **Status:** Draft, plan review pending
- **Owner:** TBD
- **Date:** 2026-11-13
- **Kind / risk:** Medium work, High risk (caching override in `AGENTS.md`;
  persisted lockfile and refs formats; shared store layout)
- **Affected crates:** `omni_remote_source`, `omni_lockfile`,
  `omni_remote_source_contributors`, `omni_api` (generator, projection, tool,
  remote_source operations), `omni_cli_core` (generator, `remote-sources prune`),
  `omni_projections` (moved-store recognition), `apps/site` docs
- **Related:** RFC 0027 (global remote-source store), RFC 0013
  (remote-sources install)

## 1. Summary

Today every layer of the remote-source store is git-shaped: the `RemoteSource`
enum, the store layout (`git/`, `trees/<slug>/<commit>`), the lockfile
(`git: {uri: {rev: {commit}}}`), the reference sets, prune entries, and eleven
call sites that build `RemoteSource::Git` by hand. This RFC separates the
kind-neutral parts (the per-remote lock, atomic publish, manifest, `.used`,
prune, pins) from the kind-specific parts (resolve and fetch, then export a
plain tree). A new kind such as an HTTP package registry then means one enum
variant, one trait impl and one config mapping, with no change to the store
core, the lockfile, refs or prune.

The registry kind itself is **out of scope**. It gets its own RFC (auth,
integrity, safe archive extraction, version ranges).

## 2. Goal and definition of done

Done when:

1. A `SourceKind` trait exists, and git is implemented on it. The store core
   does not mention git outside the git impl.
2. A second kind, a test-only fake in `omni_remote_source/tests`, materializes,
   locks, records refs, self-heals and prunes through the core. The core,
   lockfile, refs and prune code are unchanged to add it.
3. The lockfile and refs use one kind-neutral shape (section 4.3). Existing
   Files in the old shape (top-level `git` key) are read and rewritten in the
   new shape on the next save; the `version` tag stays `1.0.0`.
4. Each kind has its own namespace in the store, so two kinds whose slugs are
   the same string never share a lock, mirror or tree.
5. `SourceConfig` is mapped to `RemoteSource` in one function. No call site
   builds a variant by hand.
6. Every existing test still passes: RFC 0027 worked examples, `remote-sources`
   e2e, projections, prune.

### Out of scope

- The HTTP or npm registry kind (auth, metadata, tarballs, SRI, extraction).
- Writing the old lockfile or refs shape, or bumping their version. Breaking change accepted,
  since omni is pre-release.
- Migrating trees from the v1 store layout in place (section 4.2).
- Dependency resolution between sources.

## 3. Findings this plan builds on

- `omni_source_configurations::SourceConfig` already has an inert,
  serde-hidden `Registry(RegistrySource { name, version })` stub, so the config
  side is ready. Only `materialize_flat_git` and its siblings ignore
  non-git variants.
- `RemoteSourceManager::materialize` matches on `RemoteSource`, and the
  materialize path in `materialize_git` (`manager/default_impl.rs`) is already
  shaped as: fast-path check, then per-remote lock, then resolve, then export,
  then mark used, then lock pin. Only `resolve_commit` and
  `omni_git_utils::export_tree` are git-specific.
- `export_tree` already seals any directory in `.pending/` into a manifest, so
  sealing and publishing are kind-neutral today. Only the fill step calls git.
- `store_slug` is an injective encoding of a URL into one path segment. It is
  reusable for any kind whose locator is a URL.
- The lockfile and refs are serde-tagged enums (`version: "1.0.0"`). An
  unknown version fails to parse. Unknown *fields* are ignored today, which is
why 4.3 adds `deny_unknown_fields`
  instead of silently dropping entries it doesn't know.
- `LockfileDataV1_0_0` and `RefSetDataV1_0_0` each have exactly one field,
  `git`.
- `StoreEntry` and `StorePruneFilter::only` identify an entry as
  `(remote, commit)` with no kind.
- `omni_projections::engine` recognises its own links after the store moves
  by their `<slug>/<commit>/<file>` tail (`a_moved_store_is_recognised_by_...`).
  A tail anchored one level deeper (`<kind>/<slug>/<pin>`) keeps working.
- This repo tracks `.omni/sources/lock.json`, so step 1 produces a one-time
  diff here.

## 4. Design

### 4.1 Interfaces

```rust
/// Kind-neutral address of one source: everything the core needs, as strings.
pub struct SourceSpec {
    /// Stable lockfile/refs key for the place the source lives (git: the URL).
    pub locator: String,
    /// What the user asked for at that place (git: a rev; registry: a range).
    pub selector: String,
}

/// An immutable content id produced by `resolve`. It is used as a path segment
/// and a lockfile value. The core rejects it unless it is one safe segment
/// (no separators, not `.`/`..`, not empty).
pub struct Pin(String);

#[async_trait::async_trait]
pub trait SourceKind<TSys: RemoteSourceSys>: Send + Sync {
    /// Path segment and lockfile/refs key, e.g. "git". Must be unique, stable
    /// and a safe segment.
    fn id(&self) -> &'static str;

    /// One path segment naming the place (git: `store_slug(uri)`). Must be
    /// injective over `spec.locator`.
    fn slug(&self, spec: &SourceSpec) -> String;

    /// Turn `spec` (and the locked pin, if any) into a pin. May fetch into the
    /// kind's raw cache `raw_dir`. Called with the per-remote lock held.
    /// `locked` is `None` on first use or after `invalidate`.
    async fn resolve(
        &self,
        ctx: &KindContext<'_, TSys>,
        spec: &SourceSpec,
        locked: Option<&Pin>,
    ) -> Result<Pin, Error>;

    /// Write the plain files of `pin` into `dest`, an empty pending directory.
    /// Called with the per-remote lock held, after `resolve`. Must not touch
    /// anything outside `dest` and `ctx.raw_dir`.
    async fn export(
        &self,
        ctx: &KindContext<'_, TSys>,
        spec: &SourceSpec,
        pin: &Pin,
        dest: &Path,
    ) -> Result<(), Error>;
}

pub struct KindContext<'a, TSys> {
    pub sys: &'a TSys,
    /// `<store>/v2/raw/<kind>/<slug>/`, owned by the kind (git: the bare repo).
    pub raw_dir: PathBuf,
}
```

- `RemoteSource` stays a closed, serde enum (`Git { uri, rev }`, later
  `Registry { .. }`). It gains `fn kind_id(&self) -> &'static str` and
  `fn spec(&self) -> SourceSpec`. The manager holds the kind impls in a
  `KindRegistry` (a static list keyed by `id`) and dispatches through it.
- The manager's generic `materialize_with(kind, spec)` is public for tests, so
  a fake kind can be exercised without adding a variant to `RemoteSource`.
  `materialize(&RemoteSource)` keeps its signature, so call sites don't change
  in step 2.
- `MaterializedSource { root, pin: String }` and `RemoteSourceRef` are
  unchanged.
- `invalidate_git(uri, rev)` becomes `invalidate(&RemoteSource)`, and
  `locked_commit` becomes `locked_pin(&RemoteSource)`.

Core algorithm (moved out of `materialize_git` unchanged in behavior):

1. Look up the locked pin and validate it as one safe segment before any
   path join (lockfile loading also rejects unsafe pins in both the old and new shape
   files). If the tree is complete, mark it used and return (no lock).
2. Take `locks/<kind>/<slug>.lock` and `check_layout`.
3. `kind.resolve(ctx, spec, locked)`, then validate the pin as a safe segment.
   When `locked` is `Some`, the returned pin must equal it; any other pin is an
   error that names the kind, locator and both pins, and nothing is exported.
   Only `invalidate` (`--update`) clears a lock so a kind may re-resolve.
4. If the tree is incomplete: make a pending dir, `kind.export` into it,
   `tree::seal`, rename into `trees/<kind>/<slug>/<pin>`, write the manifest
   last. On error, remove the pending dir.
5. Mark used. If nothing was locked, lock the pin.

### 4.2 Store layout v2

```text
<store>/
  VERSION                              # stays "1"; v1 omni keeps working
  git/ trees/ locks/                   # v1, untouched, prunable
  v2/
    VERSION                            # "2"
    locks/<kind>/<slug>.lock
    raw/<kind>/<slug>/                 # kind-owned (git: bare repo)
    trees/<kind>/<slug>/<pin>/
    trees/<kind>/<slug>/<pin>.manifest
    trees/<kind>/<slug>/<pin>.used
    .pending/<pid>-<nanos>-<seq>/
```

Decision: put v2 under `<store>/v2/` and leave v1 untouched.

| Option | Cost | Chosen |
| --- | --- | --- |
| `v2/` subdirectory beside v1 | One refetch per machine (reduced by the v1 seed below). v1 data lingers until pruned or deleted. | Yes: no migration code, older omni on the same machine keeps working, matches RFC 0027's "different layout paths per version" |
| Migrate v1 in place (rename `git/` to `raw/git/`, `trees/<slug>` to `trees/git/<slug>`) | Needs every remote lock across versions. An older omni then refuses the store | No |
| Bump root `VERSION` to 2 and discard v1 | Breaks any older omni on the machine and races its readers | No |

**Recovering pins the remote no longer serves.** A v2 store starts empty, so a
locked commit that was force-pushed away or garbage-collected on the remote
would be unfetchable. Whenever a locked commit is absent from
`v2/raw/git/<slug>` (whether that mirror is missing or already exists) and the
v1 bare mirror `<store>/git/<slug>.git` exists, the git kind copies that
commit from v1 before any network fetch:

- It transfers the closure of the **locked commit itself**, not the objects
  reachable from v1's refs, since a force-pushed commit can stay in v1 while no
  ref reaches it. Preferred: a local fetch with an explicit want of the commit
  id. If the local transport refuses an unadvertised id, copy the commit's
  reachable objects from v1's object database into v2's. No alternates file,
  so v2 never depends on v1 surviving.
- It records the commit as `refs/omni/pins/<commit>` in v2, so a later
  `+refs/heads/*` fetch never leaves it unreachable. This ref is written
  **last**, only after the whole closure is in v2, and it is the completeness
  marker: a commit counts as present in v2 only when a ref reaches it (a
  normal fetch updates refs after its pack lands; the copy writes the pin ref
  after its objects). Commit presence alone never counts, so a copy
  interrupted after the commit object but before its trees and blobs is simply
  redone on the next run (copying an object that already exists is a no-op).
- It runs under the v1 `locks/<slug>.lock` (and the v2 per-remote lock), so it
  never reads a mirror a v1 omni is writing.

The network fetch runs only if the commit is still missing. When neither
mirror holds it, the error names the remote and the commit and says to run
`omni remote-sources install --update` to re-lock, which changes the pin.
v1 is only read, never written.

`omni remote-sources prune` covers v2 only. The docs say that the v1
directories can be deleted with `rm -rf` (the store is a cache) only when no
omni process of any version is running on the machine: an older omni uses
them, and a newer omni reads `git/` and `locks/` while copying a commit into
v2, so deleting them mid-copy makes the copy fail (it then falls back to the
network, or to re-locking). Deleting them also drops the v1 seed, so a
locked commit that the remote no longer serves and v2 has not yet copied can
then be recovered only by re-locking, which changes its pin. An older omni
cannot read v2, so after deletion it cannot recover such a commit even when v2
holds it; keep the v1 directories while any older omni is still in use. Run
`omni remote-sources install` in each workspace first to copy what they pin. A
`--legacy` flag is not added (nothing would be left after a release).

### 4.3 Lockfile and refs: new shape, version stays `1.0.0`

```json
{
  "version": "1.0.0",
  "sources": {
    "git": {
      "https://example.com/a.git": { "main": { "pin": "c1…" } }
    }
  }
}
```

Shape: `sources: Map<kind, Map<locator, Map<selector, { pin }>>>`, sorted at
every level, for both `lock.json` and `refs/<id>.json`. The `version` tag is
not bumped: omni is pre-release and the owner accepts the break, so
`LockfileDataV1_0_0` and `RefSetDataV1_0_0` change shape in place.

- **Read:** the new shape. A file that still has the old top-level
  `git: {<uri>: {<rev>: {commit}}}` key is converted to
  `sources.git.<uri>.<rev>.pin` on load. Without this, serde would ignore the
  unknown `git` key and the next save would silently drop every pin.
- **Write:** the new shape only, so the old `git` key disappears on the next
  save.
- Unknown fields (`#[serde(deny_unknown_fields)]`) and an unknown kind under
  `sources` are load errors that name the field or kind, never ignored:
  ignoring them would delete a newer omni's pins on save.
- An older omni reading a new-shape file fails (its struct requires `git`); it
  never silently overwrites it.
- API: `Lockfile::{pin(kind, locator, selector), lock_pin(..), pins() ->
  Vec<(kind, locator, selector, pin)>, remove(..)}`. The `git_*` methods are
  removed.

Decision (owner: breaking change allowed, no version bump):

| Option | Cost | Chosen |
| --- | --- | --- |
| Change the `1.0.0` shape in place, convert the old `git` key on load | One-time diff; older omni can't read the file | Yes: no version machinery for a pre-release format; a new kind needs no format change; the converter is a few lines and avoids silently re-resolving every branch |
| Bump to `2.0.0` and keep a `1.0.0` reader | A second tagged variant to carry for a format that has no released users | No |
| Change the shape, no converter | Users must delete the lockfile and every pin re-resolves to the current branch head; worse, without `deny_unknown_fields` the old pins are silently dropped | No |

### 4.4 Call sites and prune

- One function, `omni_remote_source_contributors::remote_source_for(&SourceConfig)
  -> Option<RemoteSource>`, returns `None` for `Local`. `Registry` stays
  unreachable (serde-skipped) until its RFC. It replaces the 11 `RemoteSource::Git
  { .. }` constructions in `omni_api` (generator, projection, tool,
  remote_source), `omni_remote_source_contributors` (lib, pack, projection) and
  `omni_cli_core` (generator).
- `StoreEntry` gains `kind`, and `StorePruneFilter::only` takes
  `(kind, remote, pin)`. The prune output and its MCP/JSON form add a `kind`
  column.
- `omni_projections` moved-store recognition anchors on
  `<kind>/<slug>/<pin>/<file>`. For a link into a v1 tree, it also accepts
  `<slug>/<commit>/<file>` with an implied kind of `git`, so `sync` repairs a
  v1 link instead of treating it as foreign.

### 4.5 Sandbox

No change. The read grant is the directory a tool or generator loads from,
which is now `v2/trees/<kind>/<slug>/<pin>`.

## 5. Worked example (becomes the test fixture)

Fixtures: git repo R (commits c1, c2). Fake kind `fake` whose locator is a
local directory of numbered snapshots and whose selector is a snapshot name.
It resolves to the snapshot's content hash and exports by copying. Workspaces
P and U share one store.

Happy path:
1. P installs R@main with an existing old-shape lockfile pinning c1. No network
   (c1 in mirror) once `v2/raw/git/<R>` exists. Otherwise it is one fetch.
   The tree is at `v2/trees/git/<R>/c1`. The lockfile is rewritten as
   in the new shape (`version` still `1.0.0`) with `sources.git.<R>.main.pin = c1`, and the refs file the same.
2. P materializes `fake:<dir>@s1` via `materialize_with`. The tree is at
   `v2/trees/fake/<slug>/<hash>`, the lockfile gains `sources.fake...`, and
   `remote-sources prune --dry-run --all` lists both entries with their kinds.

Complexities:
3. P and U concurrently materialize the same fake spec: one export, both
   succeed (same per-remote lock path `v2/locks/fake/<slug>.lock`).
4. A git slug and a fake slug that are the same string do not collide (separate
   `<kind>` directories and locks).
5. A projection symlink made under the v1 layout
   (`<store>/trees/<R>/c1/a.md`), after `rm -rf <store>/trees`: `status`
   reports it broken. `sync` recognises it as its own and relinks to
   `v2/trees/git/<R>/c1/a.md`.
6. An older omni on the same machine still reads and writes `<store>/git` and
   `<store>/trees` (v1 `VERSION` unchanged).

Edge cases and failure paths:
7. The fake kind returns a pin `../x`, `a/b` or an empty string: the
   materialize error names the kind and the pin, and nothing is written outside
   `.pending`.
8. The fake kind's `export` fails halfway: the pending dir is removed, no tree,
   no lock entry.
9. A deleted fake tree with an intact raw dir is re-exported. A failed
   `resolve` with no raw dir gives an error that names the kind and locator.
10. A lockfile with `sources.registry` (unknown here), or with an unknown
    top-level field: load fails with
    an error that names `registry`, and the file is unchanged.
11. `v2/VERSION` = 3: refuse, as in RFC 0027.
12. `--update` invalidates through `invalidate(&RemoteSource)` for both kinds.
13. A lockfile pinning `../x` (or `a/b`): load fails naming the entry, before
    the fast path joins any path.
14. With a locked pin, the fake kind's `resolve` returns a different pin: error
    naming both pins; no tree exported, lockfile unchanged.
15. Git pin c0 exists only in the v1 mirror and no v1 ref reaches it
    (force-pushed away on the remote): the v2 install copies c0 from v1 and
    succeeds offline, both when `v2/raw/git/<R>` is missing and when it already
    exists without c0. Without the v1 mirror: error naming the remote and pin
    and pointing at `--update`.
16. The v1 copy is interrupted after c0's commit object but before its trees
    (fault injected in the test): v2 has c0 but no `refs/omni/pins/c0`, so the
    next install redoes the copy, writes the ref and exports c0.

## 6. Steps

| # | Outcome | Depends on | Check | Undo |
| --- | --- | --- | --- | --- |
| 1 | Lockfile and refs new shape under `1.0.0` (convert the old `git` key on load, write the new shape, unknown field or kind is an error), generic `Lockfile` API. Git callers adapted; `.omni/sources/lock.json` regenerated | none | Round-trip, old-shape conversion and unknown-kind and unsafe-pin unit tests (examples 10, 13). All existing tests and `remote-sources` e2e pass | Revert, then restore `lock.json` from git. A new-shape file elsewhere can be deleted and reinstalled (pins re-resolve) |
| 2 | `SourceKind` trait, `KindRegistry`, generic core (`materialize_with`), git impl, v2 layout under `<store>/v2/`, pin validation, projection tail recognition | 1 | Fake kind integration tests (examples 2–4, 7–9, 11, 14–16). RFC 0027 examples. Example 5 and 6 tests. Projection moved-store tests | Revert. `v2/` is orphaned and harmless; v1 data was never touched |
| 3 | `remote_source_for` mapping replaces the 11 constructions; `invalidate`/`locked_pin`; kind-aware `StoreEntry`, prune filter and output | 2 | `rg 'RemoteSource::Git \{'` finds only the mapping and the git impl. Prune unit and e2e tests with `kind` (example 12). CLI and MCP snapshots updated | Revert |
| 4 | Docs: store reference (v2 layout, v1 cleanup), lockfile shape, release note (breaking), contributor pattern "adding a source kind" in `docs/patterns/` | 1–3 | Site build. The pattern doc walks through the fake kind | Revert |

The riskiest part is step 1, which changes a git-tracked format and touches
every git caller. It goes first so the rest builds on settled data.

Step 2 is next riskiest: concurrency and caching, with a second reviewer. It
keeps `materialize(&RemoteSource)` stable, so steps 2 and 3 can each be
reverted alone.

Steps are sequential, so this is solo work with no split. Integration is
continuous.

## 7. Risks

| Risk | Likelihood / impact | Mitigation |
| --- | --- | --- |
| Old-to-new shape conversion loses or alters a pin | Low / high (silent re-resolve) | Conversion tests on real fixture files. Step 1 diff of `.omni/sources/lock.json` reviewed by hand: every pin is the same |
| A newer omni's kind is dropped by an older omni on save | Medium / high | An unknown kind is a load error, never ignored (example 10) |
| A kind returns an unsafe pin or slug, escaping the store | Low / high | The core validates both as single safe segments before any path join (example 7) |
| A kind writes outside `dest` and `raw_dir` | Low / medium | Documented contract. The fake kind test asserts nothing else changed. The registry RFC must cover extraction safety |
| Refetch storm after upgrade (v2 starts empty) | High / low | One-time per machine, and skipped where the v1 mirror seeds v2. Documented in the release note |
| Locked pin no longer served by the remote after upgrade (force-push, GC) | Low / medium | Seed from the v1 mirror (4.2, example 15); otherwise the error says to re-lock with `--update`, which changes the pin |
| A kind returns a pin other than the locked one, so the tree and lockfile disagree | Low / high | The core rejects it (4.1 step 3, example 14) |
| Projection links into v1 trees not recognised after v1 cleanup | Medium / low | Legacy tail accepted (example 5) |
| v1 directories waste disk | Medium / low | Release note and store reference say `rm -rf <store>/{git,trees,locks}` is safe only when no omni process of any version is running (a newer omni reads v1 while copying into v2), and warn that a locked commit the remote no longer serves and v2 has not yet copied is then lost (recover only by `--update`, which changes the pin); advise `install` in each workspace first |

## 8. Assumptions and open questions

Assumptions:
- gix can fetch an unadvertised commit id over the local file transport, or
  copy one commit's object closure between two bare repos. Checked by a spike
  at the start of step 2 (the spike picks between the two; both meet 4.2) and also runs example 16.
- Every future kind can express its address as two strings (locator, selector)
  and its content as one immutable pin string. This holds for git and npm
  (locator `registry-url/name`, selector a range, pin a version plus integrity
  encoded as a segment). The registry RFC is where this assumption gets checked.
- `async_trait` dyn dispatch with a generic `TSys` stays object-safe, as
  `RemoteSourceContributor` already is.
- No external crate depends on `Lockfile::git_*` or `invalidate_git` outside
  this workspace.

Decided with the owner:
1. Breaking lockfile change accepted (pre-release). The shape changes in place
   and the version stays `1.0.0`; the old `git` key is converted on load.
2. The registry kind is out of scope, and extensibility is proven with a fake
   kind.

Open questions: none that block step 1.

## 9. Verification and review

- Per step: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`
  on touched crates (commands in `CONTRIBUTING.md`).
- The section 5 examples become integration tests in
  `omni_remote_source/tests` and `omni_api/tests`.
- High risk: plan review before step 1. Second reviewer on steps 1 and 2.
- Each PR description: what, why, risks, alternatives, how to undo.

## 10. Status

- [ ] Plan review
- [ ] Step 1: lockfile and refs new shape
- [ ] Step 2: `SourceKind` core, git impl, v2 layout
- [ ] Step 3: single mapping, kind-aware prune
- [ ] Step 4: docs
