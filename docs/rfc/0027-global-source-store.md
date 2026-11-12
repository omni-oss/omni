# RFC 0027: Global remote-source store

- **Status:** Draft — open questions resolved; plan review pending
- **Owner:** TBD
- **Date:** 2026-11-11
- **Kind / risk:** Medium work, High risk (caching override in `AGENTS.md`;
  shared machine-wide state, concurrency, deletion)
- **Affected crates:** `omni_remote_source`, `omni_git_utils`, `omni_api`
  (remote_source, projection, generator, tool, pack operations),
  `omni_configurations`, `omni_context`, `omni_projections`, `omni_cli_core`,
  `omni_capability_enforcement` (read grants), `apps/site` docs
- **Related:** RFC 0013 (remote-sources install), RFC 0017 (worktree cache
  sharing)

## 1. Summary

Move the remote-source store (`.omni/sources/store`) out of each workspace into
one per-user store, as bun, pnpm and npm do with their package caches. The
default lives in the platform cache directory; `OMNI_SOURCES_DIR` and
`worktree.omni.{ext}` `sources.dir` override it. The store is a cache: any part
of it may be deleted at any time, and omni refetches what is missing. Lockfiles
and reference sets stay per workspace.

## 2. Goal and definition of done

Done when:

1. Two workspaces (worktrees or unrelated clones) pinning the same commit fetch
   it once and share it on disk.
2. Deleting any part of the store, or all of it, never breaks a later
   `omni` command: tools, generators and packs refetch on next use (network
   permitting; offline from the bare repo where it survives). Projections whose
   links are owned files (reflink, copy) are unaffected; a projection that is a
   symlink into the store is flagged by `projection status` and repaired by
   `projection sync`, not silently on every read.
3. No command run in one workspace deletes data another workspace needs, except
   the explicit prune command, and the effect of that is recoverable by (2).
4. The store location follows the precedence in section 4.1.

### Out of scope

- Sharing a store between machines or over network filesystems.
- Readers of the store written by older omni versions (old per-workspace
  stores are left in place and ignored).
- Changing lockfile, reference-set, or projection ledger formats.
- Task cache (`.omni/cache`) behavior; RFC 0017 is unchanged.

## 3. Findings this plan builds on

- `RemoteSourceManager::retain()` prunes every store checkout not listed in the
  *current* workspace's `refs/*.json`, under a lock that lives beside the
  workspace lockfile. A throwaway test with two workspaces on one store root
  showed `retain()` in A deleting B's checkout. **Sharing the current store
  without changing GC is unsafe.**
- Publishing is already concurrency-safe: clone into `store/pending/<unique>`,
  then rename into place, tolerating a racing winner.
- `materialize_git` already refetches a locked commit whose directory is
  missing. Tools, generators and packs therefore self-heal today.
- Projections in `LinkKind::Auto` prefer a **relative symlink** into the store
  (`omni_projections/src/apply.rs`). A deleted store entry leaves dangling links.
  `engine.rs` status already detects dangling links but `sync` does not repair
  them from a refetch.
- Each commit is currently a full clone with its own `.git`, so history is
  duplicated per pinned commit.
- `directories::ProjectDirs::from("com", "omni-oss", "omni")` is already used in
  `omni_setup`.
- Sandboxed tasks (landlock, seatbelt, AppContainer) get read grants scoped
  around the workspace. A projection symlink resolving into `~/.cache/omni`
  would be unreadable inside the sandbox. **Not covered by the earlier
  assessment; added here.**
- Rebased onto PR #81 (workspace ignore rules vs fetched sources). It carries
  source locality through expansion by pin (`Locality` in
  `omni_remote_source_contributors`), explicitly not by path, and discovery over
  a fetched source already skips workspace ignores. Moving the store outside
  the workspace needs no change there; existing fixtures and the
  `remote-sources` e2e spec must pass unchanged with a global store root, and
  step 1 adds a check that a git source in a store outside the workspace still
  ignores workspace rules.

## 4. Design

### 4.1 Location and precedence

First applicable source wins:

1. Explicit: `--sources-dir` CLI flag, `OMNI_SOURCES_DIR`, or API builder input.
2. Current workspace `worktree.omni.*` `sources.dir`.
3. Primary workspace `worktree.omni.*` `sources.dir` (RFC 0017 discovery).
4. Global default: `ProjectDirs::cache_dir()/sources`
   - Linux: `$XDG_CACHE_HOME/omni/sources` (`~/.cache/omni/sources`)
   - macOS: `~/Library/Caches/com.omni-oss.omni/sources`
   - Windows: `%LOCALAPPDATA%\omni-oss\omni\cache\sources`
5. If no home/cache directory can be determined: `<workspace>/.omni/sources/store`
   (today's location), with a notice.

Path rules are RFC 0017 section 3.2 unchanged: relative paths resolve against
the workspace whose file supplied the value; `~` and variable forms are an
actionable error (decided: no new expansion rules; use the env var for
home-relative paths). The unsafe-location guard of RFC 0017 section 3.3 applies.

Schema addition, validated like `cache`:

```yaml
# worktree.omni.yaml — machine-local
sources:
  dir: ../shared-sources
```

Resolution reuses `CacheResolutionState` in `omni_context`: a second resolved
path with its own `SourcesDirectorySource` enum, computed once per context, from
the same single read of the worktree files.

### 4.2 Store layout

```text
<store>/
  VERSION                      # "1"; unknown version => refuse with a notice
  locks/<slug>.lock            # advisory lock per remote
  git/<slug>.git/              # bare repo per remote, objects shared
  trees/<slug>/<commit>/       # plain export of one commit, no .git
  trees/<slug>/<commit>.manifest # files and sizes; the completeness marker
  trees/<slug>/<commit>.used   # mtime = last use (for prune)
  .pending/<pid>-<nanos>-<seq>/ # in-flight exports, renamed into trees/
```

- Fetch: under `locks/<slug>.lock` (required: unlocked concurrent fetches
  fail with a reference race, see section 9), fetch with explicit refspecs
  into the bare repo (incremental after the first clone).
- Export: under the same per-remote lock as the fetch, check out the commit into
  `.pending/`, rename it to `trees/<slug>/<commit>`, then write its manifest.
  Validation of an existing tree and recovery of an incomplete one also run under
  that lock, so a reader never removes a tree an exporter is still publishing; the
  only lock-free path is a read of a tree whose manifest already validates.
- The completeness manifest lives **beside** the tree, as
  `trees/<slug>/<commit>.manifest`, never inside it: a repository may track a
  root file with any name (including `.manifest`), and the export must be exactly
  what the commit contains. It is written after the tree is renamed into place
  (atomically, via a temporary file), so a tree without a manifest reads as
  incomplete and is re-exported.
- Read: a reader that finds `trees/<slug>/<commit>` checks it against the
  manifest (every listed path exists with the recorded size; a stat per file, no
  hashing). If it passes, use it and touch `.used`. If the manifest is missing or
  any file is missing or the wrong size (a cleaner deleted part of the tree),
  discard the tree and re-export from the bare repo under the remote's lock.
  Missing bare repo => fetch, and if that fails the error names the remote and
  says the network is needed.
- Published files are made read-only, so a hard link or a file symlink into the
  tree fails on an in-place write instead of altering shared content.
  **Directories stay writable on purpose**: the store must be safe to delete with
  ordinary tools (`rm -rf`, a cache cleaner), and read-only directories would make
  that fail. The consequence is that a *directory* symlink into a tree could add
  or replace files, and a same-size replacement passes the manifest check.
  Therefore `auto` never links a directory from the store (it reflinks per file or
  copies, see 4.4), and the remaining exposure is explicit `link: symlink` or
  `link: hardlink` on a store directory, which the docs call out as sharing the
  tree. `prune` and re-export delete through the same ordinary calls.

Steps 1–3 keep today's `git/<slug>/<commit>` full-clone layout under the new
root, so the location change and the format change can each be reverted alone;
step 4 switches to the layout above. Nothing in the sandbox grant (4.5) names a
layout: it is the directory a tool or generator is loaded from, so it is correct
before and after step 4.

### 4.3 Garbage collection

- `omni remote-sources install` **no longer deletes from the store.** `retain()`
  keeps pruning the workspace lockfile pins and reference sets only.
- New `omni remote-sources prune [--older-than <dur>] [--larger-than <size>]
  [--all] [--dry-run]`, dry-run by default like `omni cache prune`. Deletes trees
  by `.used` age, then bare repos with no remaining trees. Takes each per-remote
  lock before removing that remote's entries.
- No automatic prune in this RFC.

### 4.4 Projections

Explicit and automatic link methods are separated.

- **Explicit `link:` values are cross-platform only:** `auto`, `symlink`,
  `hardlink`, `copy`. An explicit value does exactly that method or returns an
  error. It never falls back, and never swaps in a platform-specific method
  (no hidden junction for `symlink`, no hidden reflink for `copy`).
- **Situational selection is for `auto` only.** `auto` picks among reflink
  (copy-on-write clone: APFS, Btrfs/XFS/ZFS, ReFS), hardlink, symlink, NTFS
  junction (directories on Windows) and copy, using whatever the OS,
  filesystem, volume and source type (file or directory) allow. Reflink and
  junction are never named in config.
- **`junction` is removed from `LinkKind`** outright, with no alias or
  deprecation, since omni is pre-release. Configs that set `link: junction`
  fail schema validation. Remove it from the config schema, JSON schema and
  `apply.rs`; the junction call stays only as an internal `auto` fallback.

`LinkKind::Auto` order becomes, for sources under the store root only:

- **file:** reflink → symlink (absolute path into the store) → copy;
- **directory:** reflink per file → copy. A directory is **never linked** into the
  store: a directory symlink would let the workspace add or replace files in the
  shared tree (see 4.2). On Windows a junction is the same live link, so it is not
  used for store directories either; it remains only in the local-source order.

`auto` never hardlinks a store source: a hard link shares the inode, so an
in-place edit of the projection would change the tree every other workspace
reads. Each step is attempted only when the situation allows it and falls
through to the next otherwise (only on the allowlisted situational errors, see
section 10). Copies and reflinks of store files are made writable, since they
belong to the workspace. Explicit `symlink` links into the store with absolute
targets (relative links break when a workspace moves). Local (in-workspace)
projection sources keep today's Auto order.

Directory sources: directories cannot be hard linked on Linux, macOS or
Windows. For a directory source, reflink and an explicit `hardlink` therefore
operate per file: omni creates the directories, then clones or hard links each
file. If any file cannot be linked (for example the store is on another volume),
the partial tree is removed: `auto` then copies, and an explicit `hardlink`
returns the error. Implementation note: the ledger keeps one entry per
destination (a per-file directory is one entry), and `status` checks the
destination exists; the per-file set the draft proposed was not needed because
reflinked and copied files are owned by the workspace.

Rationale: reflink gives independent owned files that survive cache deletion
(goal 2) and need no sandbox grant. Where it is unavailable (for example ext4 or
NTFS), a *file* becomes a symlink into the store rather than a copy, so it costs
no disk space; a *directory* is copied. Users who want owned files everywhere set
`link: copy`. File symlinks can dangle after a prune or cache deletion;
`status` reports them and `sync` repairs them.

`omni projection sync` re-applies a link whose destination no longer resolves
even when the pin is unchanged (the commit is refetched before the link is
applied), and recognises its own link after the store moved (`sources.dir`
changed): a git checkout keeps its `<slug>/<commit>/<file>` tail wherever the
store lives, so the old link is repaired rather than treated as foreign.

### 4.5 Sandbox

Only the JS runtimes that run tools and generators are sandboxed (Deno flags,
Landlock, Seatbelt, AppContainer); tasks and projections are not, so a symlink
projection needs no grant. A confined runtime must still read the entrypoint it
loads, which used to sit inside the `@workspace/**` floor and, with a store
outside the workspace, does not.

The store is shared across workspaces and may hold private repositories fetched
by another workspace, so the store root is never granted. Instead, when
enforcement is on and a tool entrypoint (or generator script) lies outside the
workspace, the spawn policy additionally grants read to exactly that directory
(`SpawnPolicy::grant_read`: Deno `--allow-read=<dir>` plus the OS-sandbox read
subtree). The grant is fixed when the runtime is spawned, so such runtimes are
pooled per directory; tools inside the workspace keep sharing one process.
Because the grant is the directory the code is loaded from, it is correct under
either layout and nothing else in the store becomes readable. A path that cannot
be passed safely in a flag (`,`, `=`, control characters) is not granted and the
runtime fails loudly instead.

**Known gap:** on Windows the AppContainer-confined Deno runtime dies with `EPIPE`
when its entrypoint is in a store outside the workspace, so those two e2e cases
are skipped there until the grant is worked out on a Windows machine. Capability
enforcement is experimental and off by default; without it, tools and generators
from the store run on Windows.

Verified end to end on Linux with Deno under capability enforcement (tool and
generator `run-javascript` from a git source in a store outside the workspace);
the e2e tests run on the macOS and Windows lanes too, which is where the seatbelt
and AppContainer behaviour gets verified.

## 5. Worked example (becomes the integration test fixture)

Setup: repo R with commits c1, c2. Primary workspace P at `/w/p`, linked worktree
L at `/w/l`, unrelated clone U at `/w/u`. `HOME=/h`, Linux.

Happy path:
1. In P, `omni remote-sources install` pins R@main=c1. Store is
   `/h/.cache/omni/sources`; `trees/R/c1` exists; P's lock and refs record c1.
2. L's `.omni/sources/lock.json` is tracked in git, so L already records
   R@main=c1 (same commit as P). Install in L uses that pin: no network fetch,
   and it reuses `trees/R/c1`. A worktree with no lock entry resolves R@main
   through the remote like any fresh workspace; that case is not this example.
3. P projects a file from R with Auto: on Btrfs/APFS it is a reflink; on
   ext4/NTFS (or another volume) a symlink into the store; a copy only if
   symlinks are also unavailable. A directory is reflinked per file or copied.
   Never a hardlink.

Complexities:
4. P and U install concurrently, both needing a new commit c2 of R: one fetch
   into `git/R.git`, two racing exports, one wins, both succeed.
5. U sets `sources.dir: ../alt` in its `worktree.omni.yaml`; U uses `/w/alt`, P
   and L are unaffected. L with no local value inherits P's value.
6. A sandboxed tool task in P that runs from `trees/R/c1` can read it.

Edge cases and failure paths:
7. `rm -rf /h/.cache/omni/sources`, then `omni run` of a tool in L: refetches c1
   and runs. P's reflinked projection still has its content.
8. A symlink-kind projection in P after the tree is deleted: `projection status`
   reports broken; `projection sync` refetches and repairs it.
9. Same as 7 while offline, bare repo intact (only `trees/` removed): re-export
   without network. Bare repo also gone: clear error naming the remote and that
   network is needed.
10. Install in P never deletes `trees/R/c2`, which only U uses.
11. `sources.dir: ~/x` is rejected with the RFC 0017 expansion error.
    `sources.dir: /w/p` (the workspace) is rejected by the unsafe-location guard.
12. Store `VERSION` from a newer omni: commands refuse to write and say so.

## 6. Alternatives rejected

| Option | Why not |
| --- | --- |
| Shared store with per-workspace GC (status quo widened) | Proven to delete other workspaces' checkouts. |
| Registration/lease files for cross-workspace GC | Works but adds persistent shared state to keep correct; unnecessary once the store is a disposable cache. |
| Collect refs via `git worktree list` | Misses unrelated clones and non-git setups. |
| Keep symlink-first Auto into the global store | Dangling after any prune even where a reflink was possible. Symlink is now tried only after reflink, before copy. |
| Hardlink in `auto` | Shares the inode with the store tree, so an in-place edit of a projection changes what every workspace reads. |
| `~/.omni` as default | Not the platform convention for disposable data; excluded from nothing (backups, cleaners). |

## 7. Steps

Each step leaves omni working and is reviewable alone.

| # | Outcome | Depends on | Check | Undo |
| --- | --- | --- | --- | --- |
| 0 | Regression test: two workspaces on one store root; `install` in A must not delete B's entries (fails today) | — | Test fails before step 1, passes after | Delete test |
| 1 | `install` stops deleting store entries; store root resolved by precedence (4.1) with `sources.dir`, env var, flag; per-remote lock; current layout | 0 | Unit tests for precedence and path rules; examples 1, 2, 5, 10, 11; test 0 | Revert; old per-workspace stores are untouched |
| 2 | Shared test helper and per-lane required capabilities (section 10); projections: explicit values limited to auto/symlink/hardlink/copy (`junction` removed from `LinkKind`, schema and docs updated); store-sourced Auto = file: reflink → symlink → copy, directory: reflink per file → copy (never hardlink, never a directory link); absolute targets for explicit symlinks; `sync` repairs dangling/missing links and links of a moved store; spawn-time read grant for the directory a confined tool or generator loads from | 1 | Examples 3, 6, 7, 8 on Linux, macOS, Windows CI | Revert; existing links keep working, new syncs return to old order |
| 3 | `omni remote-sources prune` with `.used` markers | 1 | Dry-run lists, real run deletes, refetch after (7); concurrent prune vs install | Revert; store just grows |
| 4 | Layout v1: bare repo + trees + `VERSION` | 1, 3 | Examples 4, 9, 12; disk use measured on a repo with 2 pins | Revert; old layout directory is a different path, both can coexist |
| 5 | Docs: reference page, `worktree.omni` reference, release notes | 1–4 | Site build | Revert |

Riskiest part: step 2 (link-kind change and sandbox grants touch two High-risk
areas). It runs right after the prerequisite step 1 so it is learned early.
Step 4 is the second riskiest (gix bare fetch + checkout on Windows); a spike
before starting it is allowed to reorder 3 and 4.

## 8. Risks

| Risk | Likelihood / impact | Mitigation |
| --- | --- | --- |
| Concurrent prune removes a tree a running task reads | Low / task error | Prune is explicit only; document; retry on next run refetches |
| Reflink and hardlink unavailable → Auto symlinks into the store, which dangle after prune or cache deletion | Medium / low | `status` flags dangling links, `sync` refetches and repairs; users wanting immunity set `link: copy` |
| An in-place edit through a shared inode or symlink changes the store tree | Medium / medium | `auto` never hardlinks and never links a directory; published files are read-only; the manifest check re-exports a tree whose file is missing or resized; explicit `symlink`/`hardlink` on a store directory is documented as sharing it |
| Read-only files make `rm -rf` of a tree print errors on some platforms | Low / low | Directories stay writable, so deletion works without chmod; only an attempt to edit a file in place fails |
| Sandbox grant exposes other workspaces' (possibly private) sources | Low / medium | Grant only the directory a confined tool or generator loads from, read-only, never the store root; second reviewer from capabilities owners |
| Multiple omni versions on one machine | Medium / medium | `VERSION` file; different layout paths per version |
| Platform tests skip silently on CI and prove nothing | Medium / high | Section 10: required-capability lists per lane make a skip a failure; the `SKIPPED[` marker is printed in the job log |
| Global store on a slow or network home dir | Low / medium | `OMNI_SOURCES_DIR` / `sources.dir` override |

## 9. Assumptions and open questions

Pre-build experiments (throwaway code, Linux/Btrfs, kernel 7.2, git 2.56, gix
0.90; code deleted, results kept here):

| Assumption | Result |
| --- | --- |
| `gix` can build a bare repo and export a commit's tree without `.git` | Confirmed. `prepare_clone_bare` + `fetch_only` brings all branches and tags (as `refs/heads/*`, `refs/tags/*`), commits on non-default branches included. Export via `index::State::from_tree` + `worktree::state::checkout` into a plain directory preserved the executable bit and symlinks. Works over `file://` and `https://`. Pitfall: `filter_pipeline` must be given the **tree** id, not the commit id. |
| Incremental fetch into an existing bare repo | Confirmed, but a bare clone has **no fetch refspec configured**: omni must set refspecs explicitly (`+refs/heads/*:refs/heads/*`, `+refs/tags/*:refs/tags/*`) or `receive` fails. |
| Concurrent fetches into one bare repo are safe without a lock | **Refuted.** Four threads fetching at once: three failed with a reference-update race ("reference changed"). `git fsck` stayed clean, so no corruption, but the per-remote lock in 4.2 is required. |
| Reflink of a directory | **Linux: not supported.** `reflink-copy` returns `InvalidInput` for a directory; per-file only (matches 4.4). Cross-volume reflink and hardlink both fail with `CrossesDevices`, which is detectable and drives the fall-through. A reflinked file stays independent of its source after an edit or delete; a hard-linked file shares in-place edits with the source. `filefrag` showed shared extents for a per-file reflinked tree. `reflink_or_copy` silently copies, so omni must call `reflink` and decide the fall-through itself. |
| Sandbox: links into the store need a read grant | **Confirmed (Landlock).** With read granted to the workspace only: a symlink into the store was denied; a hard link and a plain copy of the same file were allowed; direct store reads were denied. Granting the store root made the symlink readable. So 4.5 is required for symlinked projections and for anything run from the store, and not for hardlink, reflink or copy. |

Still unverified (needs macOS and Windows CI, not possible here):
- gix bare fetch and tree export on macOS and Windows (step 4 spike).
- `clonefile` on APFS, reflink on ReFS/Dev Drive, NTFS junction fallback.
- Seatbelt and AppContainer read grants for the store root (step 2).
- That a reflink crate fits behind `system_traits` without breaking
  `InMemorySys` tests (step 2; a design question, not an OS question).
- A pinned commit that no branch or tag reaches (force-pushed away): today's
  clone cannot fetch it either; the bare repo at least keeps commits it has
  already fetched.

Decisions (resolved with the owner):
1. Explicit `link: symlink` from a remote source stays allowed. `projection
   status` reports it as broken once the target is gone; `projection sync`
   refetches and repairs it. Automatic repair on every read (rejected for now)
   can be revisited if manual `sync` proves annoying, since `auto` can also
   produce symlinks. Goal 2 is worded to match.
2. The CLI flag is `--sources-dir`, matching `sources.dir`.
3. No automatic age-based prune in this RFC; only `omni remote-sources prune`.
4. `hardlink` stays an explicit value, strict (error when impossible), and
   works on directories through the per-file tree.

Open questions: none blocking. The Landlock grant is verified; seatbelt and
AppContainer remain for step 2 (second reviewer).

## 10. Platform-, filesystem- and OS-specific tests

Reflink, hardlink, symlink and junction behaviour, Landlock/seatbelt/AppContainer
grants, and gix on each OS depend on the OS, kernel, filesystem and privileges.
These tests must never pass because they silently did not run, and a failure
must name what went wrong.

1. **Two gates, used for different things.**
   - `#[cfg(target_os = ...)]` only where the code cannot compile elsewhere.
   - A runtime capability probe for everything that depends on the filesystem,
     volume, kernel or privilege (reflink on the test directory's filesystem,
     hardlink on the same volume, symlink privilege on Windows, Landlock ABI,
     junction). Probe the actual directory under test, not the OS name.
2. **One shared helper** (test-support module) replaces the per-file copies of
   `should_run_confined`. `require(Capability::X, dir)` returns available or
   unavailable. When unavailable:
   - locally: skip and print a stable marker,
     `SKIPPED[capability=reflink fs=ext4 dir=/tmp/x]`;
   - when `OMNI_TEST_REQUIRE_CAPABILITIES` lists it (comma list, or `all`): **panic**
     with `REQUIRED CAPABILITY UNAVAILABLE: reflink fs=ext4 dir=/tmp/x`.
   The existing `OMNI_REQUIRE_OS_SANDBOX` stays as an alias for the
   `os-sandbox` capability.
3. **Each CI lane declares what it must prove.** Implemented in
   `.github/workflows/run-omni-jobs.yaml` (the multiplatform job sets
   `OMNI_TEST_REQUIRE_CAPABILITIES` per target):
   - Linux (x86_64): `symlink,hardlink,git,reflink`. Reflink runs on a Btrfs loop
     volume mounted at `/mnt/omni-reflink` (`OMNI_TEST_REFLINK_DIR`); default
     runner disks are ext4. aarch64 Linux tests run under `cross` in a container
     that sees neither the volume nor the variables, so they require nothing.
   - macOS: `symlink,hardlink,git,reflink` (APFS temp dir).
   - Windows: `symlink,hardlink,git`; the runners offer no ReFS volume, so reflink is
     not required there and rule 4 covers the fall-back.
   - Targetless jobs (the default Linux runner, which runs crates that are not
     multiplatform) require `symlink,hardlink,git`.
   - `os-sandbox` is not required on any lane yet: the Seatbelt and AppContainer
     tests live in `omni_capability_enforcement`, which only runs on the Linux job
     today (only `cli/omni` has per-platform test lanes). Making that crate
     multiplatform and requiring `os-sandbox` there is a follow-up that should be
     checked on real runners first.
   Only `cli/omni` has per-platform test lanes, and its `test` task depends on the
   `test` of every crate it uses, so a crate's gated tests run on each platform
   lane as part of that closure; the required list is part of the test task's
   cache key. (A dedicated multiplatform preset for `omni_projections` was tried
   and dropped: it grew the dry-run summary past the argument-size limit of the
   `determine-build-tools` action.) A skipped test prints a `SKIPPED[...]` line in
   the job log; a required capability that is missing fails the job.
4. **Fallbacks are asserted, not skipped.** Where a capability is absent the
   test asserts the fallback: on a non-reflink filesystem `auto` yields
   symlink, then copy with the expected `ResolvedKind`; a
   second directory on another volume (tmpfs, `/dev/shm`) must produce the
   cross-volume fall-through. These run on every lane.
5. **Fall through only on known situational errors.** `auto` moves to the next
   method only for an allowlist: `Unsupported`/`EOPNOTSUPP`, cross-device
   (`CrossesDevices`), invalid-for-this-filesystem from the clone call, and
   Windows `ERROR_PRIVILEGE_NOT_HELD`. Anything else (permission denied, out of
   space, not found, I/O) is returned as an error, never swallowed. `auto`
   returns which method won and why each earlier one was skipped, so tests
   assert the reason, and a debug trace records it.
6. **Failures name the cause.** Assertion messages include OS, filesystem type,
   directory, capability and the underlying error kind.

## 11. Verification and review

- Per step: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` on
  touched crates (`omni run <task> -p <project>`).
- Example section 5 as integration tests in `omni_api/tests` and
  `omni_remote_source/tests`.
- High risk: plan reviewed before step 1; a second reviewer for steps 2 and 4.
- Description of each PR: what, why, risks, alternatives, how to undo.

## 12. Status

- [x] Plan review (CodeRabbit; findings folded in)
- [x] Step 0: regression tests
- [x] Step 1: store location, `sources.dir`, `--sources-dir`, `OMNI_SOURCES_DIR`, per-remote lock, no deletion on install
- [x] Step 2: capability gate and lane wiring; link kinds; dangling-link repair; sandbox grant
- [x] Step 3: `omni remote-sources prune`
- [x] Step 4: bare mirror + trees + `VERSION`
- [x] Step 5: user docs (store reference, `remote-sources prune` page)
- [ ] macOS and Windows verification on CI (the platform lanes now run the link tests)
