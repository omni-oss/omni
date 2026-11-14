# RFC 0031: Pluggable remote-source kinds

- **Status:** Accepted
- **Owner:** TBD
- **Date:** 2026-11-13
- **Kind / risk:** Medium work, High risk (caching override in `AGENTS.md`;
  persisted lockfile and refs formats; shared store layout)
- **Affected crates:** `omni_remote_source`, `omni_lockfile`,
  `omni_remote_source_contributors`, `omni_api` (generator, projection, tool,
  remote_source operations), `omni_cli_core` (generator, `remote-sources prune`),
  `omni_projections` (moved-store recognition), `omni_meta` (`expand.rs`, pin
  naming only), `apps/site` docs
- **Related:** RFC 0027 (global remote-source store), RFC 0013
  (remote-sources install)

## 1. Summary

Today every layer of the remote-source store is git-shaped: the `RemoteSource`
enum, the store layout (`git/`, `trees/<slug>/<commit>`), the lockfile
(`git: {uri: {rev: {commit}}}`), the reference sets, prune entries, and eleven
non-test call sites that build `RemoteSource::Git` by hand. This RFC separates
the kind-neutral parts (the per-remote lock, atomic publish, manifest, `.used`,
prune, pins) from the kind-specific parts (resolve and fetch, then export a
plain tree). A new kind such as an HTTP package registry then means one enum
variant, one driver and one config mapping, with no change to the store
core, the lockfile, refs or prune.

omni is pre-release, and the owner allows breaking changes to the store
layout, the lockfile and the refs files **without changing any version
marker and without compatibility code**. Nothing in this RFC reads, converts
or seeds from the old formats.

The registry kind itself is **out of scope**. It gets its own RFC (auth,
integrity, safe archive extraction, version ranges).

## 2. Goal and definition of done

Done when:

1. A `SourceDriver` trait exists, and git is implemented on it. The store core
   does not mention git outside the git driver.
2. A second kind, a test-only fake in `omni_remote_source/tests`, materializes,
   locks, records refs, self-heals and prunes through the core. The core,
   lockfile, refs and prune code are unchanged to add it.
3. The lockfile and refs use one kind-neutral shape (section 4.3). A file in
   the old shape fails to load with an error; there is no converter.
4. Each kind has its own namespace in the store, so two kinds whose slugs are
   the same string never share a lock, mirror or tree.
5. `SourceConfig` is mapped to `RemoteSource` in one function. No call site
   builds a variant by hand.
6. Every existing test still passes, updated to the new layout and shapes:
   RFC 0027 worked examples, `remote-sources` e2e, projections, prune.

### Out of scope

- The HTTP or npm registry kind (auth, metadata, tarballs, SRI, extraction).
- Any compatibility with the current layout, lockfile or refs: no reader or
  converter for the old shapes, no seeding from old mirrors, no recognition of
  projection links into old trees, no version bumps.
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
- The lockfile and refs are serde-tagged enums (`version: "1.0.0"`). Unknown
  _fields_ are ignored today, so without `deny_unknown_fields` an old-shape
  file would load as empty and the next save would silently drop its pins
  (4.3).
- `reconcile_pins` reads every `refs/*.json`, and `persist_pins` reloads and
  saves the lockfile on every install (`manager/default_impl.rs`).
- `StoreEntry` and `StorePruneFilter::only` identify an entry as
  `(remote, commit)` with no kind.
- `omni_projections::engine::same_store_file` recognises its own links after
  the store moves by comparing the last two components of the source root
  (`<slug>/<commit>`).
- The fetch receives the configured URL, credentials included
  (`fetch_bare(.., uri.as_str(), ..)`), while lockfile keys are the full `Url`
  and can contain userinfo.
- This repo tracks `.omni/sources/lock.json`, so step 1 regenerates it.

## 4. Design

### 4.1 Interfaces

```rust
/// Kind-neutral address of one source: everything the core needs, as strings.
pub struct SourceSpec {
    /// Stable lockfile/refs key for the place the source lives, in canonical
    /// form and without credentials (git: see "canonical locator" below).
    pub locator: String,
    /// What the user asked for at that place (git: a rev; registry: a range).
    pub selector: String,
    /// The address as configured, credentials included (git: the URL from
    /// the source config, userinfo intact). In memory only: never serialized,
    /// never part of a lockfile or refs key, never used for slugs.
    /// `resolve` and `export` connect with this, not with `locator`.
    /// `Connection` is a new newtype in `omni_remote_source` with no
    /// `Serialize` and a redacting `Debug`/`Display`.
    pub connection: Connection,
}

/// An immutable content id produced by `resolve`, in canonical form (git: the
/// full lowercase SHA). Used as a path segment and a lockfile value, and
/// compared byte for byte with a locked pin. The core rejects it unless it is
/// a *safe segment*: `[a-z0-9._-]`, 1 to 128 bytes, no leading `.`, and no
/// `.manifest` or `.used` suffix (those names belong to the sibling markers).
/// Lowercase only, so it cannot collide on a case-insensitive filesystem.
/// Kinds encode anything else (e.g. a registry encodes a base64 integrity as
/// lowercase hex).
pub struct Pin(String);

#[async_trait::async_trait]
pub trait SourceDriver<TSys: RemoteSourceSys>: Send + Sync {
    /// Path segment and lockfile/refs key, e.g. "git". Must be unique, stable
    /// and a safe segment.
    fn kind(&self) -> &'static str;

    /// One path segment naming the place (git: `store_slug(uri)`). Must be
    /// injective over `spec.locator`. The core makes it safe: a slug that is
    /// too long, not lowercase-safe, or starts with `h-` is replaced by
    /// `h-<blake3 hex of the slug>`. Only hashed slugs start with `h-`, so a
    /// hashed and a literal slug never collide.
    fn slug(&self, spec: &SourceSpec) -> String;

    /// Turn `spec` (and the locked pin, if any) into a pin. May fetch into the
    /// kind's raw cache `raw_dir`. Called with the per-remote lock held.
    /// `locked` is `None` on first use or after `invalidate`. When `locked` is
    /// `Some` and the raw cache already has it, must not use the network.
    /// When `ctx.offline`, must not use the network at all.
    async fn resolve(
        &self,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        locked: Option<&Pin>,
    ) -> Result<Pin, Error>;

    /// Write the plain files of `pin` into `dest`, an empty pending directory.
    /// Called with the per-remote lock held, after `resolve`. Must not touch
    /// anything outside `dest` and `ctx.raw_dir`.
    async fn export(
        &self,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        pin: &Pin,
        dest: &Path,
    ) -> Result<(), Error>;
}

pub struct DriverContext<'a, TSys> {
    pub sys: &'a TSys,
    /// `<store>/raw/<kind>/<slug>/`, written by the kind (git: the bare repo).
    /// Deletion belongs to the core (prune), not the kind.
    pub raw_dir: PathBuf,
    /// Placeholder for the registry RFC: no network allowed.
    pub offline: bool,
    /// Placeholder for the registry RFC (credentials, scope mapping). Empty for
    /// git. Credentials come from here or from `spec.connection`, never from
    /// the locator.
    pub config: &'a DriverConfig,
}
```

- Naming: a _kind_ is the id string (`"git"`, `"fake"`) used as a path
  segment and lockfile key; a _driver_ is the `SourceDriver` implementation
  for one kind.
- `SourceDriver`, `DriverContext` and `DriverConfig` are `#[doc(hidden)]` and
  unstable until the registry RFC settles credentials, offline policy and
  per-pin metadata (such as an integrity to verify on export).
- **Canonical git locator**, one function `canonical_git_locator(&Url) ->
  String` used for every `SourceSpec`: parse with `url` (lowercases scheme and
  host, drops a default port), remove userinfo, strip one trailing `/` from the
  path, then `as_str()`. A `.git` suffix is kept.
- **A locator names one content source.** URLs that differ only in
  userinfo are the same source: they share a lockfile entry, a slug and a
  tree. This is today's behaviour (`store_slug` already drops credentials).
  For git it is safe for content, since a commit id fixes the content
  regardless of account. It does mean a pinned tree already in the store is
  reused without a fetch, so it is not re-checked that a second account can
  read the repository; the store is per OS user, so this only shares data
  between workspaces of the same user. Servers that serve different content
  per account under one URL are unsupported. A driver whose content can
  differ per account must put a non-secret discriminator (for example an
  account or scope name, never a token) in its locator.
- Credentials still reach the fetch: the git kind connects with
  `spec.connection`, the configured URL with userinfo intact, exactly as
  `fetch_bare` receives it today. It is redacted in logs and errors and never
  written anywhere.
- `RemoteSource` stays a closed, serde enum (`Git { uri, rev }`, later
  `Registry { .. }`). It gains `fn kind(&self) -> &'static str` and
  `fn spec(&self) -> SourceSpec`. The manager holds the drivers in a
  `DriverRegistry` keyed by `kind()`; production builds register the built-in kinds,
  and tests can register more. That registry is also the set of kinds a
  lockfile may contain (4.3).
- The manager's `materialize_with(kind, spec)` is `#[doc(hidden)]` and exists
  for tests, so a fake kind can be exercised without adding a variant to
  `RemoteSource`. `materialize(&RemoteSource)` keeps its signature, so call
  sites don't change in step 2.
- `MaterializedSource { root, pin: String }` and `RemoteSourceRef` are
  unchanged.
- `invalidate_git(uri, rev)` becomes `invalidate(&RemoteSource)`, and
  `locked_commit` becomes `locked_pin(&RemoteSource)`.

Core algorithm (moved out of `materialize_git` unchanged in behavior):

1. Look up the locked pin and validate it as one safe segment before any path
   join (lockfile loading also rejects unsafe pins). If the tree is complete,
   mark it used and return (no lock).
2. Take `locks/<kind>/<slug>.lock` and `check_layout`.
3. `kind.resolve(ctx, spec, locked)`, then validate the pin as a safe segment.
   When `locked` is `Some`, the returned pin must equal it; any other pin is an
   error that names the kind, locator and both pins, and nothing is exported.
   Only `invalidate` (`--update`) clears a lock so a kind may re-resolve.
4. If the tree is incomplete: make a pending dir, `kind.export` into it,
   `tree::seal`, rename into `trees/<kind>/<slug>/<pin>`, write the manifest
   last. On error, remove the pending dir.
5. Mark used. If nothing was locked, lock the pin.

A test asserts the core module does not import `omni_git_utils`, so DoD 1 is
checked mechanically.

### 4.2 Store layout

The layout changes in place at the store root. `VERSION` stays `1`.

```text
<store>/
  VERSION                              # "1", unchanged
  locks/<kind>/<slug>.lock
  raw/<kind>/<slug>/                   # kind-written (git: bare repo)
  trees/<kind>/<slug>/<pin>/
  trees/<kind>/<slug>/<pin>.manifest
  trees/<kind>/<slug>/<pin>.used
  .pending/<pid>-<nanos>-<seq>/
```

- Nothing reads the current `git/`, `trees/<slug>/` or `locks/<slug>.lock`
  entries. They are dead cache: listing and prune walk only
  `trees/<registered kind>/` and `raw/<registered kind>/`, so old entries are
  neither listed nor deleted. The release note says to `rm -rf` the old
  `git/` directory and any `trees/`/`locks/` children that are not a kind id.
  Old and new names cannot collide: old slugs start with the encoded scheme
  (`https_3a…`) and kind ids are short words.
- After upgrading, every pinned source is fetched once more. A locked commit
  the remote no longer serves (force-pushed away, garbage-collected) fails
  with an error naming the remote and the commit and saying to run
  `omni remote-sources install --update`, which re-locks and changes the pin.
- An older omni using the same store is not supported (it does not
  understand the new layout).
- `omni remote-sources prune` removes a remote's raw dir once its last tree is
  gone, as today.

Decision:

| Option                                                          | Cost                                                                          | Chosen                                  |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------- |
| New layout in place, `VERSION` unchanged, no compatibility code | One refetch per machine; commits the remote no longer serves need `--update`  | Yes: owner allows the break; least code |
| `v2/` subdirectory plus seeding from the old mirror             | Two-lock protocol, object-closure copy, completeness markers, cleanup caveats | No: compatibility only                  |

### 4.3 Lockfile and refs shape

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
every level, for both `lock.json` and `refs/<id>.json`. The `version` tag
stays `1.0.0`; `LockfileDataV1_0_0` and `RefSetDataV1_0_0` change shape in
place.

- `sources` is required and always written, even as `{}`. A fresh workspace
  has no file, not an empty one.
- `#[serde(deny_unknown_fields)]` on the data structs and entries. An old-shape
  file (top-level `git`) therefore fails to load (unknown field `git` or
  missing field `sources`) instead of loading as empty and losing its pins on
  save. There is no converter: the release note says to delete
  `.omni/sources/lock.json` and `.omni/sources/refs/` and run
  `omni remote-sources install`, which re-resolves every pin.
- The lockfile crate does **not** know the set of kinds: `sources` is a plain
  `Map<String, Map<String, Map<String, Entry>>>`. The manager checks every kind
  against its `DriverRegistry` after load, and an unknown kind is an error that
  names it, never dropped (dropping would delete its pins on save). Tests
  register the fake kind, so adding a kind never edits `omni_lockfile`.
- Tests run whole `{"version":"1.0.0", …}` documents through the tagged enum,
  since `deny_unknown_fields` and internally tagged enums need checking
  together (`flatten` is not used).
- API: `Lockfile::{pin(kind, locator, selector), lock_pin(..), pins() ->
  Vec<(kind, locator, selector, pin)>, remove(..)}`. The `git_*` methods are
  removed, and `sorted_view` is rewritten for three levels.

Decision:

| Option                                               | Cost                                                                    | Chosen                                             |
| ---------------------------------------------------- | ----------------------------------------------------------------------- | -------------------------------------------------- |
| New shape in place, no converter, old files rejected | Users re-run install once; pins re-resolve                              | Yes: owner allows the break; no compatibility code |
| Convert the old `git` key on load                    | Converter, merge and conflict rules for a format with no released users | No: compatibility only                             |

### 4.4 Call sites and prune

- One function, `omni_remote_source_contributors::remote_source_for(&SourceConfig)
  -> Option<RemoteSource>`, returns `None` for `Local`. `Registry` stays
  unreachable (serde-skipped) until its RFC. It replaces the 11 non-test
  `RemoteSource::Git { .. }` constructions (match arms not counted) in
  `omni_api` (generator, projection, tool, remote_source),
  `omni_remote_source_contributors` (lib, pack, projection) and `omni_cli_core`
  (generator).
- `StoreEntry` gains `kind`, and `StorePruneFilter::only` takes
  `(kind, remote, pin)`. The prune output and its MCP/JSON form add a `kind`
  column.
- `omni_projections` `same_store_file` compares three components,
  `<kind>/<slug>/<pin>`, so a `fake` and a `git` source with equal slug and pin
  strings never match each other's links. Links into the old layout are not
  recognised: `projection status` reports them, and the release note says to
  delete them and run `projection sync`.
- Every git-specific site is changed in step 4, not only constructions: the
  dedup key `HashSet<(Url, String)>` in `omni_api` `remote_source.rs`; the
  `(Url, String)` keys in `reconcile_pins` and `prune_lockfile_pins`; the
  `git_pin`/`from_store` naming in `omni_api` `projection.rs` and
  `omni_projections` `engine.rs`; `omni_meta` `expand.rs` (`pin` naming).

### 4.5 Sandbox

No change. The read grant is the directory a tool or generator loads from,
which is now `trees/<kind>/<slug>/<pin>`.

## 5. Worked example (becomes the test fixture)

Fixtures: git repo R (commits c1, c2). Fake kind `fake` whose locator is a
local directory of numbered snapshots and whose selector is a snapshot name.
It resolves to the snapshot's content hash and exports by copying. Workspaces
P and U share one store.

Happy path:

1. P installs R@main with no lockfile. One fetch into `raw/git/<R>`, tree at
   `trees/git/<R>/c1`, lockfile `sources.git.<R>.main.pin = c1`, refs file the
   same shape. A second install makes no network call.
2. P materializes `fake:<dir>@s1` via `materialize_with`. The tree is at
   `trees/fake/<slug>/<hash>`, the lockfile gains `sources.fake…`, and
   `remote-sources prune --dry-run --all` lists both entries with their kinds.

Complexities: 3. P and U concurrently materialize the same fake spec: one export (the fake
`export` call count is 1), both succeed (same lock `locks/fake/<slug>.lock`). 4. A git slug and a fake slug that are the same string do not collide
(separate `<kind>` directories and locks), and projection links of one are
not recognised as the other's. 5. Old-layout entries (`git/<slug>.git`, `trees/<old slug>/c1`) present in the
store: they are not listed, not pruned, not read; install fetches into the
new layout.

Edge cases and failure paths: 6. The fake kind returns a pin `../x`, `a/b`, `A1` or an empty string: the
materialize error names the kind and the pin, and nothing is written
outside `.pending`. 7. The fake kind's `export` fails halfway: the pending dir is removed, no tree,
no lock entry. 8. A deleted fake tree with an intact raw dir is re-exported. A failed
`resolve` with no raw dir gives an error that names the kind and locator. 9. Load errors, each leaving the file bytes unchanged, for `lock.json` and a
`refs/*.json`: (a) old shape (top-level `git`), (b) unknown entry field,
(c) `sources.registry` with no `registry` kind registered (error names
`registry`; manager check), (d) version-only file with no `sources`. 10. `VERSION` = 2: refuse, as in RFC 0027. 11. `--update` invalidates through `invalidate(&RemoteSource)` for both kinds. 12. A lockfile pinning `../x` (or `a/b`): load fails naming the entry, before
the fast path joins any path. 13. With a locked pin, the fake kind's `resolve` returns a different pin: error
naming both pins; no tree exported, lockfile unchanged. 14. Config URLs `https://host/r.git/`, `https://HOST:443/r.git` and
`https://user@host/r.git` all give the locator `https://host/r.git`. 15. A git source configured with userinfo (a local test server that requires
it) fetches, and neither the lockfile, the refs file, the slug nor any log
line contains the credential. 16. A locked git commit the remote no longer has, with an empty store: error
naming the remote and commit and pointing at `--update`. 17. A git remote whose slug is not lowercase-safe (uppercase path) gets the
slug `h-…`; a fake kind returning the literal slug `h-abc` is hashed too.

## 6. Steps

| #   | Outcome                                                                                                                                                                                                                                                                      | Depends on | Check                                                                                                                                                                                                           | Undo                                                |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| 1   | Lockfile and refs new shape under `1.0.0` (`sources` keyed by plain strings, `deny_unknown_fields`, no converter), canonical git locator, `Connection` carrying credentials to the fetch, generic `Lockfile` API. Git callers adapted; `.omni/sources/lock.json` regenerated | none       | Round-trip and load-error unit tests (examples 9a, b, d, 12, 14), credentials reach the fetch and are never persisted (15). All tests, updated, and `remote-sources` e2e pass                                   | Revert and restore `lock.json` from git             |
| 2   | `SourceDriver` trait, `DriverRegistry`, generic core (`materialize_with`), git driver, new layout in place, pin and slug safe-segment rule, manager-side unknown-kind check                                                                                                  | 1          | Fake kind integration tests (examples 2–8, 9c, 10, 13, 16, 17), core-does-not-import-git test. RFC 0027 examples on the new layout                                                                              | Revert; entries in the new layout become dead cache |
| 3   | Projections: three-component tail                                                                                                                                                                                                                                            | 2          | Example 4 (projection half); projection moved-store tests updated                                                                                                                                               | Revert                                              |
| 4   | `remote_source_for` mapping replaces the 11 constructions; `invalidate`/`locked_pin`; kind-aware `StoreEntry`, prune filter and output; every git-specific site in 4.4                                                                                                       | 2          | `rg 'RemoteSource::Git \{' --glob '!**/tests/**'` finds only the mapping, the git driver and the `RemoteSource` dispatch arms. Prune unit and e2e tests with `kind` (example 11). CLI and MCP snapshots updated | Revert                                              |
| 5   | Docs: store reference (layout), lockfile shape, release note (breaking: re-run install, delete old store entries and old projection links), contributor pattern "adding a source kind" in `docs/patterns/`                                                                   | 1–4        | Site build. The pattern doc walks through the fake kind                                                                                                                                                         | Revert                                              |

The riskiest part is step 1, which changes a git-tracked format and touches
every git caller. It goes first so the rest builds on settled data. Step 2 is
next (concurrency and caching) and gets a second reviewer. Step 2 keeps
`materialize(&RemoteSource)` stable, so steps 2, 3 and 4 can each be reverted
alone.

Steps are sequential, so this is solo work with no split. Integration is
continuous.

## 7. Risks

| Risk                                                                                                                                             | Likelihood / impact | Mitigation                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| An old-shape lockfile loads as empty and loses its pins on save                                                                                  | Low / high          | `deny_unknown_fields` and a required `sources` make it a load error (example 9a)                                                                                |
| An unknown kind is dropped on save                                                                                                               | Low / high          | The manager rejects any kind missing from its `DriverRegistry` right after load, before anything can save (9c)                                                  |
| Pins re-resolve after upgrading (users delete the lockfile)                                                                                      | High / low          | Expected break; the release note says so. Users who need the old commits note them before upgrading                                                             |
| A locked commit the remote no longer serves is lost with the old store                                                                           | Low / medium        | Error points at `--update` (example 16); release note                                                                                                           |
| Credentials leak into the lockfile, refs, slug or logs                                                                                           | Low / high          | Locator strips userinfo; `Connection` has no `Serialize` and redacts (example 15)                                                                               |
| Two configured URLs differing only in userinfo share one entry and tree, so the second account's access is not re-checked when the pin is cached | Low / low           | Same as today; one OS user's store. Documented in 4.1; per-account content is unsupported, and drivers needing it put a non-secret discriminator in the locator |
| Authenticated fetches stop working once the locator drops userinfo                                                                               | Low / high          | The fetch uses `spec.connection` (example 15)                                                                                                                   |
| A kind returns an unsafe pin or slug, escaping the store or colliding on a case-insensitive filesystem                                           | Low / high          | The core validates pins and makes slugs safe before any path join (examples 6, 12, 17)                                                                          |
| A kind writes outside `dest` and `raw_dir`                                                                                                       | Low / medium        | Documented contract. The fake kind test asserts nothing else changed. The registry RFC must cover extraction safety                                             |
| A kind returns a pin other than the locked one, so the tree and lockfile disagree                                                                | Low / high          | The core rejects it (4.1 step 3, example 13)                                                                                                                    |
| Old store entries waste disk                                                                                                                     | Medium / low        | Release note gives the paths to delete                                                                                                                          |

## 8. Assumptions and open questions

Assumptions:

- Every future kind can express its address as two strings (locator, selector)
  and its content as one immutable pin string. This holds for git and npm
  (selector a range, pin a version plus integrity encoded as lowercase hex).
  The npm locator must not depend on a machine's registry mirror, or lockfile
  keys would differ between machines; the registry RFC defines it (for
  example the package name plus a logical registry name resolved through
  `DriverConfig`) and checks this assumption.
- No supported git server serves different content per account under one
  URL; a commit id fixes the content anyway.
- `async_trait` dyn dispatch with a generic `TSys` stays object-safe, as
  `RemoteSourceContributor` already is.
- No external crate depends on `Lockfile::git_*` or `invalidate_git` outside
  this workspace.

Decided with the owner:

1. Breaking changes to the store layout, lockfile and refs are allowed without
   changing any version marker, and no compatibility code is kept.
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

- [x] Plan review
- [x] Step 1: lockfile and refs new shape, canonical locator, credentials
- [x] Step 2: `SourceDriver` core, git driver, new layout
- [x] Step 3: projection tail
- [x] Step 4: single mapping, kind-aware prune
- [x] Step 5: docs

### Implementation notes

Built in one PR as separate commits per step. Differences from the text
above, none changing behaviour:

- `Pin` is a plain `String` in the trait, validated by the core with
  `is_safe_segment`; a newtype added nothing the check does not already give.
- Hashed slugs use blake3 (already a workspace dependency), not sha256.
- `remote_source_for_git(&GitSource)` sits beside `remote_source_for` for call
  sites that have already matched the `Git` variant; it is the only place a
  `RemoteSource::Git` is built from config.
- The prune views rename `commit` to `pin` alongside the new `kind` field.
- Example 15 (an authenticated local git server) is covered by unit tests
  instead: the locator strips userinfo, `Connection` redacts it, and refs
  written for a URL with credentials contain none. The fetch still receives
  the configured URL through `spec.connection`.
