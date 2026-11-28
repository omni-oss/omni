# Plan: route raw filesystem calls through `*Sys` traits

- **Kind / risk:** Medium work, High risk (cache storage and invalidation, `omni_cache`; AGENTS.md override).
- **Status:** Implemented; independent review passed. Delete after merge. Pattern doc (step 2), Group A (step 3), Group B (step 4) and Group B2 (step 4b) are done and verified. Group C (step 4c) is done except `omni_cli_core` and `omni_scm`, which are documented exceptions. PR #202 is merged, so its canonicalize case is closed. Delete or distill into `docs/patterns/sys-abstraction.md` when done.

## Goal

Production code that already has a `*Sys` (or raw `system_traits`) value in reach uses it for filesystem calls instead of `std::fs` or `tokio::fs`, and `docs/patterns/sys-abstraction.md` records the rule so new code follows it.

Done means:

- No raw `std::fs`/`tokio::fs` call remains in the in-scope sites below, except the documented exceptions.
- The pattern doc is merged and indexed in `docs/patterns/README.md`.
- `cargo check --workspace --tests`, clippy, fmt and the crate tests pass (commands from `CONTRIBUTING.md`).

## Scope

In scope, only where a Sys is already a parameter, a `self.sys` field, or `self.context.sys()`:

| Group | Site |
| --- | --- |
| A | `omni_git_utils::fetch_bare` (`remove_dir_all`, `rename`) |
| A | `omni_tool/bridge_runner.rs::run_js` (done); ~~`omni_generator/script_runner.rs::run_scripts`~~ (exception) |
| A | ~~`omni_projections/apply.rs::make_owned_writable`~~ (moved to exceptions; sync helper) |
| A | ~~`omni_task_executor/batch_executor.rs::replay_cached_results`~~ (exception: no async open) |
| B | `omni_cache/.../hybrid.rs`: methods `force_prune_inner`, `prune_stale_staging_dirs`, `get_many`, `get_stats`, `cache_many`, then the free helpers `restore_staged`, `remove_entry_dir`, `read_metadata_bounded`, `discard_invalid_entry`, `is_regular_file`, `load_stats` (thread `sys` in) |
| B | `omni_cache` helpers called from `hybrid.rs` (which has `self.sys`): `cache/impls/partition.rs` (`reclaim_stale_unknown`, `changed_within`, `children`), `storage/publish.rs`, `storage/records.rs` (`write_atomic`, `read_bounded`), `cache/impls/last_used_db.rs`, `cache/impls/origin.rs` |
| C | `bridge_rpc_runner/src/vendor.rs` (done: `VendorSys`, 9 calls routed) |
| C | ~~`omni_cli_core/src/task_output_capture.rs` and `subscriber.rs`~~ (exception: no Sys in `CliSubscriber`, plus the streaming create-new open gap) |
| C | ~~`omni_scm/src/checkout.rs`~~ (exception: sync `gix`-driven discovery, no sync Sys in scope) |

Out of scope (decided, to be listed as exceptions in the pattern):

- Poor candidates, kept as raw-call exceptions: `omni_setup/fallback_store/*` and `omni_utils/lock.rs` (real files and OS locks; `FsFileLock` is sync and not on the async bounds), `bridge_rpc_runner/runner.rs` (sandbox temp dir on the real system temp), `omni_tracing_subscriber` (runs before a Sys exists), `omni_cache/cache/impls/verify.rs` (signing key reads, security-sensitive, no Sys nearby).
- `omni_capability_sys::resolve_real_path` (security boundary, intentional raw calls) and `omni_task_executor/cache_restore.rs` (`cap_std` on purpose).
- `omni_cli_core/commands/mcp.rs::is_stdout_path` (device identity check, no sys parameter).
- `omni_remote_source` canonicalize: already handled by PR #202.

## Findings from the gap check (step 1)

Recorded after reading the call sites:

- `omni_projections/apply.rs::make_owned_writable` is now an exception: the helper is sync and its caller only bounds the async traits. The sync `FsSetPermissions` exists but takes a Unix `mode`, and no metadata trait exposes a portable read-only bit.
- `omni_task_executor/batch_executor.rs::replay_cached_results` is now an exception: there is no async streaming open.
- `omni_generator/script_runner.rs::run_scripts` is now an exception: it has no `sys` (the handle is captured inside the boxed factory), and `GeneratorSys`/`TransactionSys` have no canonicalize, which would need overlay semantics first.
- `omni_tool/bridge_runner.rs` was feasible with the sync `FsCanonicalize` bound (no async `map_bases` needed).
- `README.md` index updated with the new pattern.

## Assumptions and open questions

- Assumption: `RealSys` async behavior equals `tokio::fs` for these ops (`system_traits/src/impls/asynch/real_tokio.rs` wraps it directly). Verified by reading.
- Checked now: async traits cover `canonicalize`, `metadata`, `symlink_metadata`, `exists`, `read_dir`, `remove_dir_all`, `remove_dir`, `remove_file`, `rename`, `copy`, `create_dir[_all]`, `read`, `write`. Sync `FsOpen`/`FsRename`/`FsRemoveFile` exist (used by `atomic_file.rs`).
- **Gaps (the riskiest unknown):** no async streaming open (`FsOpenAsync`; sync `FsOpen` exists), no async chmod/permissions (sync `FsSetPermissions`/`FsFileSetPermissions` exist, Unix-mode based), no async file lock (sync `FsFileLock` exists). Each is either a new trait in `system_traits` (which would make this Large) or a documented exception.
- Answered: `bridge_runner` uses the sync `FsCanonicalize` bound; `GeneratorSys` has no canonicalize, hence the `script_runner` exception.
- Answered: #202 merged before this work, so it is the example in the pattern doc.

## Impact

- Trait bounds change in `omni_git_utils` (`GitUtilsSys`, previously `FsCreateDirAllAsync` only, now plus `FsMetadataAsync + FsRemoveDirAllAsync + FsRenameAsync`), possibly `omni_tool`, `omni_generator`, `omni_task_executor`, `omni_projections`. Public generics, so callers (`omni_api`, `omni_cli_core`, `omni_mcp_core`) may need bound lists widened.
- `hybrid.rs` helper signatures change (crate-private).
- No data format, config, CLI or release-note change. Directory walks keep `DirEntry` no-follow semantics. Four former `tokio::fs::try_exists` sites keep follow semantics via `try_exists_following` (restore skip, `get_stats` project output, last-used DB, `local` partition). Three keep no-follow deliberately: `discard_invalid_entry` and `force_prune_inner` now unlink a dangling entry link rather than skip it; the metadata-file existence check in `get_stats` reads a dangling link as invalid metadata and skips it with a warning, the same outcome as before. Each says so in a comment.

## Approach

Prefer, in order: the crate's `*Sys` supertrait; raw `system_traits` traits as bounds; a helper that calls `std::fs` directly only when the traits lack the operation, or its async form, with a comment saying so (as in `default_impl.rs` before #202). Do not add new `std::fs`/`tokio::fs` helpers beside a Sys that could do the job. The same rule goes into the pattern doc.

Rejected: adding new `system_traits` ops (`FsOpenAsync`, `set_permissions`) in this change. It grows the change into another crate; do it separately only if the gaps matter.

## Steps

Each step leaves the tree working and is its own commit.

1. **Gap check (first, riskiest).** Done; see Findings above.
2. **Pattern doc.** Done: `docs/patterns/sys-abstraction.md` plus the index line in `docs/patterns/README.md`.
3. **Group A.** Done: `omni_git_utils` (`fetch_bare`, `GitUtilsSys` bound), `omni_tool` (`LazyToolRunner`, `FsCanonicalize` bound). `omni_generator`, `omni_projections` and `omni_task_executor` moved to exceptions per Findings.
4. **Group B (High risk).** Done for `hybrid.rs`: methods using `self.sys` and the helpers (`read_validated_entry`, `read_metadata_bounded`, `is_regular_file`, `remove_entry_dir`, `discard_invalid_entry`, `restore_staged`, `still_uploadable`, `load_stats`) now take `sys`. 176 `omni_cache` tests pass. Check: `omni_cache` tests, including cache restore, prune and stale-staging tests; add a test if the touched path (`prune_stale_staging_dirs`, `discard_invalid_entry`) has none. Name the failure guarded against: a wrong cache hit or an entry wrongly removed or kept. Undo: revert the commit.
4b. **Group B continued (High risk, `omni_cache`).** Done: `partition` (`existing_partitions`, `reclaim_stale_unknown`, `changed_within`), `storage/publish`, `storage/records` (`write_atomic`; `read_bounded` stays raw for the streaming-open gap), `last_used_db` (stores `sys`), `origin::read_marker`. Remaining raw in `omni_cache`: `cache_archive.rs` sync extraction inside `spawn_blocking`, `verify.rs` signing-key reads, `records.rs::read_bounded`.
4c. **Group C (Low risk).** Done for `bridge_rpc_runner/vendor.rs`: new `VendorSys` (create-dir-all, metadata, read, remove-dir-all, write), and `ensure`/`is_up_to_date`/`resolve_target` take it. `omni_tool` passes its sys and gained the `FsCreateDirAllAsync` bound; `omni_generator` passes `TransactionSys::real_sys()`, because overlay writes stay buffered until commit and the spawned child reads the real filesystem. `omni_cli_core` (no Sys in `CliSubscriber`, streaming create-new open gap) and `omni_scm` (sync `gix`-driven discovery) are exceptions, commented at the call sites.
5. **Integration.** One full workspace run (check, clippy, fmt, tests) after all steps; update the pattern doc's example paths. Independent review of the whole change (Medium+ risk). Note: `cargo clippy -p omni_git_utils --tests -- -D warnings` fails on a pre-existing `eprintln!` in `crates/omni_git_utils/tests/clone_annotated_tag.rs`, so use `--lib` there.

Dependencies: 1 before 3, 4, 4b and 4c; 4b after 4; 2 is independent; 3, 4 and 4c are independent of each other; 5 last. Work solo; briefing and coordination would cost more than the mechanical edits save.

## Review follow-up

Independent review found a blocking symlink-follow regression and two doc errors; all are fixed:

- Directory walks now use `fs_symlink_metadata_async` so linked directories are ignored, as `DirEntry::file_type` did. Added tests: `symlinked_project_dirs_are_ignored_by_stats_and_prune`, `dangling_output_link_is_skipped_by_stats`.
- `try_exists` semantics: four sites keep them through `try_exists_following` (restore skip, `get_stats` project output, last-used DB, `local` partition). Three are deliberately no-follow, each with a comment: two unlink a dangling entry link instead of skipping it; the metadata-file check skips it with a warning, as before.
- `get_stats` walks skip a stat error instead of failing the call.
- Pattern doc corrected: sync permissions and locking exist; only async forms are missing.

Public signatures changed, all semver breaks if the crates are published: `GitUtilsSys` widens; `VendorSys` is new and `ensure` takes it (a generic parameter and a value argument); `LazyToolRunner`'s `ToolRunner` impl gained `FsCanonicalize + FsCreateDirAllAsync`; `omni_cache::storage::{publish_staged_dir, write_atomic, reclaim_stale_staging_dirs}` take a leading `&RealSys`.

## Risks

- Cache group changes behavior silently (High). Mitigation: step 4 tests, small commits, independent review.
- Bound widening ripples into many crates (Medium). Mitigation: step 1, and `cargo check --workspace --tests` per commit.
- The gaps force a `system_traits` change. Mitigation: treat as exceptions; stop and re-assess as Large if that is not acceptable.

## Verification and undo

Per step as above. Whole change undoes by reverting its commits in reverse order; no data or release impact.

## Distill afterwards

Keep the preference rule and exceptions in `docs/patterns/sys-abstraction.md`. Put the trait gaps in the backlog. Delete this plan.
