# F10 implementation plan: captured input view

**Status:** proposed for approval; no implementation started. All maintainer decisions are recorded below (including the default limits); no maintainer questions remain open. **Size:** Large (two slices after the F10.1 design). **Risk:** High. The view is a new containment boundary beside the capabilities crates ([AGENTS.md](../../../../../AGENTS.md) risk overrides).
**Design:** [F10.1](F10.1-captured-input-view-design.md) (API, evidence, authorization order, limits, errors). This plan does not repeat it; it fixes the build order, the code seams and the decisions the design left open. Facts below were read in the code at `origin/main` (`e1cdb515`).

## Scope and finish line

Slices F10.2 (tracked contained `read`/`exists`/`list` with limits) and F10.3 (revalidation, reject-on-change). Done when the [F10 acceptance list](F10-captured-input-view.md#acceptance) passes on Linux and Windows and macOS is a tested `Unsupported`. macOS stays `Unsupported` until native macOS CI proves beneath-root containment (decision 7).

Recorded maintainer decisions (see [README](README.md#decisions-for-finishing-the-standalone-features)), encoded here:

- **D2:** symlinks are configurable and **rejected by default**. The first release is reject-only; the follow mode exists only as a typed option and is not implemented (see F10.2c).
- **D5:** **macOS returns `Containment::Unsupported`** until a verified beneath-root open exists. No best-effort walk on macOS.
- **D9:** **reject** Windows alternate data streams and reserved device names.
- **D6:** `ENOTDIR` on a parent counts as `Missing`. **D8:** a case-spelling mismatch counts as a change. **D3:** an atomic replace with identical bytes is not a change.

Maintainer decisions on this plan (previously open questions):

1. **Build on `cap-std`** (not raw `libc`/`rustix`/`windows-sys`), gated on the F10.2a proof fixture. If the fixture fails, re-plan (trigger below).
2. **Symlink follow (F10.2c) is not in the first release.** Ship reject-only with the follow mode kept as a typed option for later. Windows junctions are never followed.
3. **The D9 name check lives only inside the view**, not in `WorkspacePath::parse`.
4. **Digest:** a local digest trait implemented with `blake3`. Do not reuse `omni_hasher::Hasher` (avoids `system_traits` and async dependencies).
5. **CI:** native macOS and Windows test runs are required before the view claims beneath-root containment on those OSes. Until then macOS stays `Unsupported` (D5). The F10.2a proof fixture running on the native CI jobs is what verifies it.
6. **Linux without `openat2` (or equivalent):** always `Containment::Unsupported`. No best-effort containment.

Out of scope: plugin permission schema and hook host services, the restart loop, a transactional snapshot, any change to the task hasher or its index, and edits to `omni_capabilities`, `omni_capability_sys`, `omni_capability_enforcement`, `dir_walker`, `omni_ignore_*` and `omni_glob*`. No discovery consumer is wired here; discovery adopts the view in its own plan (F10 is "usable by", not "used by").

## What the code already gives us

| Observed                                                                                                                                                                                                                                                                                                                                                 | Consequence                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/omni_task_executor/src/cache_restore.rs` already does handle-relative, no-follow filesystem work with `cap_std::fs::Dir` and `cap_fs_ext::DirExt` (workspace deps `cap-std 4.0.3`, `cap-fs-ext`; [tooling](../../../../tooling.md)). It has symlink-swap, parent-swap, junction and retained-handle-cannot-be-renamed tests on Unix and Windows. | Build the view on `cap-std` rather than hand-written `libc`/`rustix`/`windows-sys` (the F10.1 sketch). This changes F10.1's dependency list (decision 1); the proof fixture confirms it first. Reuse its test shapes.                     |
| `omni_path_utils::WorkspacePath` rejects empty, NUL, `\`, drive prefix, absolute, `.`/`..` and empty segments. It does **not** reject `name:stream` after the first segment or Windows reserved names. `omni_path_utils::escape_path_component` knows the reserved-name list (`lib.rs`) but is an escaper, not a validator.                              | D9 is a new access-time name check inside the view (F10.2a), not a `WorkspacePath` change (decision 3).                                                                                                                                   |
| `omni_capability_sys::PolicyEnforcingSys::guard` authorizes a symlink-resolved real path, then forwards to `TSys`. Its docs admit the check/open race. `CapabilityAuthorizer` (`authorize.rs`) and `omni_capabilities::evaluate` / `Request::Fs { write, path }` are the policy entry points.                                                            | The view needs `fs.read` decisions on a lexical path. Define `InputAuthorizer` in the new crate; a small adapter over `CapabilityAuthorizer` lives in the **consumer**, not in either capabilities crate, so those crates stay unchanged. |
| `omni_hasher::Hasher` (`Hasher`, `Blake3Hasher`, `DefaultHasher`); the crate also pulls `system_traits`, `rs_merkle`, `futures`, `eyre`. `omni_path_utils` already depends on `sha2`.                                                                                                                                                                    | Digest trait: prefer a local two-method trait with a `blake3` implementation, to keep `omni_input_view` a light leaf (D4, decision 4).                                                                                                    |
| `omni_hasher::project_dir_hasher::impls::utils` trusts `(mtime ms, size)` equality. `dir_walker` yields entries without capture or containment.                                                                                                                                                                                                          | Neither is touched. The view never reads the hasher index and does not use `dir_walker` (D10: list-only membership evidence).                                                                                                             |
| `omni_remote_source` (`manager/tree.rs`, `manager/pending.rs`, `driver.rs`) uses `symlink_metadata` and `is_safe_segment` for its own store paths.                                                                                                                                                                                                       | Not a consumer and not changed. Its segment rules are a separate, store-local policy; do not share code.                                                                                                                                  |

## Slices

Each slice leaves a working system, adds a crate nothing depends on yet, and is reviewed separately. Every slice is High risk: second reviewer, platform CI results attached, and no edit to a capabilities crate in the same PR.

### F10.2a: crate skeleton and thin proof fixture

**Risk:** Medium (no I/O of consequence yet), landed first so the dangerous slice is reviewable.

- New leaf crate `crates/omni_input_view` (workspace `crates/*` glob already includes it; add the `[workspace.dependencies]` entry). Dependencies: `omni_path_utils`, `cap-std`, `cap-fs-ext`, `blake3`, `thiserror`. No `system_traits`, plugin, runtime, Wasm or discovery crate.
- Types from F10.1 with no behavior: `InputLimits`, `InputError`, `ContainmentViolation`, `BudgetKind`, `Observation`, `Evidence`, `Cancellation`, `InputAuthorizer`, `ContainmentMode`.
- **Proof fixture (the thin slice):** one function opening a file beneath a `cap_std::fs::Dir` root via `WorkspacePath` segments, with `ContainmentViolation::Symlink` on any symlink component, tested with a symlink-swap loop on Linux and Windows. It settles decision 1 (does `cap-std` give a race-safe beneath-root open on the kernels and Windows versions we support?) before the API is built on it. On a Linux kernel without `openat2` or an equivalent, the fixture asserts `Containment::Unsupported` (decision 8). If `cap-std` is not race-safe, re-plan (see triggers).
- Name validation for D9 (inside the view only, decision 3): reject `:` in any segment on Windows builds, and reject reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`, with or without an extension, any case) plus trailing dot or space. Table-driven unit tests run on all OSes (pure function); the I/O behavior runs only on Windows.

**Gate:** the swap test shows no outside bytes in 10,000 iterations on each OS, and the macOS path returns `Unsupported` without reading (D5). **CI requirement:** the fixture must run on native Linux, Windows and macOS CI jobs (confirm the `omni_setup` preset covers the new crate on all three; add jobs if not). Until the macOS job exists and passes, macOS stays `Unsupported`; the same holds for Windows until its native job passes.
**Rollback:** delete the crate; nothing depends on it.

### F10.2b: tracked `read` / `exists` / `list` with limits

**Risk:** High (containment, authorization). Implements [F10.1 design](F10.1-captured-input-view-design.md#design-proposed): authorize first, then open beneath the root, `fstat` regular-file check with non-blocking open, limits checked before allocation, per-entry `list` authorization with the separate raw-scan counter, evidence kept per observation with conflict marking, cancellation recording nothing.

Interfaces and data shapes are those in F10.1 (`InputView::{open, read, exists, list, evidence}`; `Read`, `Exists`, `Listing`, `Observation`, `PathEvidence`, `InputError`). Decisions fixed here:

- `InputView::open` takes an absolute workspace root and holds one `cap_std::fs::Dir`. No API returns a `Dir`, handle or absolute path.
- Authorization uses the lexical path (root path joined with the validated suffix) because symlinks are rejected in the default mode.
- Default `InputLimits` values are the recorded maintainer decision below (accepted for now, adjustable later); the checks do not depend on the values. Limits are host-owned: plugins cannot raise them.
- `ENOENT` and `ENOTDIR` are `Missing`; `EACCES`, symlink, non-regular and other errors are `InputError` (D6, confirmed). A case-spelling mismatch is reported as a change (D8, confirmed).

**Gate:** the F10.1 F10.2 test list, plus the benchmark on a 1,000-file tree in `crates/omni_bench` against plain reads. Unchanged existing tests in `omni_capabilities` and `omni_capability_sys` prove authority was not expanded.
**Rollback:** revert the slice; the crate has no consumers.

### F10.2c: symlink policy, reject-only (D2)

**Risk:** Medium (no follow logic ships). Adds `SymlinkPolicy::{Reject (default), FollowInRoot { max_hops }}` on `InputLimits` or `InputView::open` as a typed option. Following is not in the first release (decision 2).

- `Reject` is the default and the only implemented mode. Selecting `FollowInRoot` returns a typed `Unsupported`-style error and follows nothing; no resolver, hop counter or re-authorization of resolved paths is written.
- Windows junctions and mount points are rejected and are never followed, in any mode.
- **Deferred:** the follow implementation. A later plan must resolve each link itself, cap hops, re-authorize every resolved `WorkspacePath`, reject targets that leave the root (absolute, `..` chains), never `canonicalize` then open, and record the requested path as evidence so a retargeted link is `Modified`. `cache_restore.rs` `Workspace::relative_target`/`resolve_link` are ideas only (trusted restore, not untrusted reads).

**Gate:** reject-matrix (absolute target, `..` target, loop, dangling, retarget mid-read, Windows junction) on Linux and Windows all return containment errors; selecting the follow option returns the typed error and reads nothing.
**Rollback:** remove the option; default behavior is already `Reject`.

### F10.3: revalidation and reject-on-change

**Risk:** High (decides whether stale input is accepted). `InputView::revalidate`, `Revalidation`, `Change`, `ChangeKind` per F10.1. Always re-read and re-digest tracked files; `FileIdentity` is advisory only and never skips the digest. Denial, containment or I/O failure during revalidation is an error, not a `Change`. Output truncates at `max_reported_changes`. Default consumer policy "reject on any `Changed`" is a helper, with no retry loop.

**Gate:** same-size same-mtime edit (`set_modified` restore), `Created` after `Missing`, `Deleted`, `KindChanged`, `MembershipChanged`, atomic replace with identical bytes is `Unchanged` (D3), case-spelling mismatch is a `Change` (D8), repeatability, cancellation leaves state untouched.
**Rollback:** revert the slice; F10.2 evidence remains usable without revalidation.

## Test matrix

| Case                                                               | Linux                                                                              | Windows                                                                                              | macOS                                                                    |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Path/name validation (pure)                                        | run                                                                                | run                                                                                                  | run                                                                      |
| Containment: symlink final, intermediate, dangling                 | run                                                                                | run (symlink and junction, both rejected; skip when symlink privilege is missing, with a named skip) | assert `Unsupported`, no read (until native macOS CI proves containment) |
| Swap-race loop, rename out and back                                | run (`openat2` path; kernel without `openat2` or equivalent asserts `Unsupported`) | run (held handles, no `FILE_SHARE_DELETE`; if rename succeeds, Windows is `Unsupported`)             | none                                                                     |
| Windows ADS and reserved names (D9)                                | name table only                                                                    | run with real files                                                                                  | name table only                                                          |
| Limits before allocation, FIFO/device (`NotRegular`), cancellation | run                                                                                | limits and cancellation; FIFO n/a                                                                    | `Unsupported`                                                            |
| Revalidation matrix                                                | run                                                                                | run                                                                                                  | `Unsupported`                                                            |
| Benchmark, 1,000 files                                             | run                                                                                | optional                                                                                             | n/a                                                                      |

CI matrix requirement: native macOS and Windows test runs of `omni_input_view` are required before the view claims beneath-root containment on those OSes. macOS has no test target in `ci.yaml` today except release builds (`manual-release-omni.yaml`), so adding one is part of F10.2a. The F10.2a proof fixture running on those native jobs is the verification.

## Compatibility and release notes

- No config schema, wire shape, cache key or on-disk format changes; the crate has no consumers, so there is **no user-visible change** in F10.2-F10.3. No release note for the crate itself.
- Add to the release notes only when a consumer lands: new rejection errors (`Containment`, `Denied`, budget errors) and the D2/D5/D9 behavior, as documented limits ("macOS unsupported", "symlinks rejected unless configured", "Windows streams and reserved names rejected").
- Update `docs/tooling.md` if `cap-std` is used from a second crate (it currently names only the executor's `cache_restore` module), and add `omni_input_view` to the check list in the same PR. Update the [README](README.md) F10 row and `tasks.yaml` status in each slice PR, not before.

## Re-planning triggers

Stop and re-plan if any of these happens:

- The proof fixture shows `cap-std` is not race-safe on a supported kernel or Windows version (decision 1: then raw `openat2`/handle code, a security-review-heavy path, or `Unsupported` for that platform).
- Native macOS or Windows CI cannot be added or the fixture fails there: that OS stays `Unsupported`.
- Windows rejects the held-handle rename boundary: Windows becomes `Unsupported` like macOS, and the acceptance wording changes.
- The authorizer adapter cannot express a per-entry `list` decision without changing `omni_capabilities` (that crate is High risk and not in scope).
- A consumer needs `Path` or handles from the view, or recursion beyond one level per call.
- Default limits from the benchmark make a realistic discovery pass exceed budget.
- A change is needed in `dir_walker`, glob or ignore crates (High risk; separate plan).

## Maintainer decision: default `InputLimits`

No maintainer questions remain open.

**Decision:** the maintainer accepted the proposed defaults below "for now, adjustable later". [§8.1](../../008.00-performance-and-shared-safeguards.md#81-shared-safeguards) of RFC 0024 gives no numbers (only that limits exist and are host-wide); the only numeric precedents are the §8.3 default external plugin envelope (64 MiB memory, 16 MiB read bytes and 8 MiB output per batch, 1 s per batch, 5 s aggregate) and the existing `DEFAULT_MAX_WORKSPACE_PATH_LEN` of 4096.

| Limit                   | Default | Note                                           |
| ----------------------- | ------- | ---------------------------------------------- |
| `max_path_bytes`        | 4096    | existing default                               |
| `max_path_depth`        | 64      |                                                |
| `max_file_bytes`        | 8 MiB   |                                                |
| `max_total_bytes`       | 16 MiB  | per view; the §8.3 per-batch read envelope     |
| `max_tracked_inputs`    | 4096    |                                                |
| `max_dir_entries`       | 10000   |                                                |
| `max_total_dir_entries` | 50000   |                                                |
| `max_dir_scan_entries`  | 100000  | far above the visible limits, per F10.1 rule 5 |
| `max_operations`        | 10000   |                                                |
| `max_reported_changes`  | 100     |                                                |

Limits are host-owned. The host may lower them or change the defaults later; plugins cannot raise them.

**Follow-up requirement (before F10.2b acceptance):** calibrate the defaults with the 1,000-file benchmark in `crates/omni_bench`, and include real pnpm and yarn lockfiles, because very large lockfiles may exceed `max_file_bytes`. Adjust the defaults if the benchmark shows a realistic discovery pass exceeding budget (see the re-planning triggers).
