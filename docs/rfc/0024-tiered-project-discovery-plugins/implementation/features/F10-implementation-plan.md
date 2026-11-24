# F10 implementation plan: captured input view

**Status:** proposed for approval; no implementation started. **Size:** Large (two slices after the F10.1 design). **Risk:** High. The view is a new containment boundary beside the capabilities crates ([AGENTS.md](../../../../../AGENTS.md) risk overrides).
**Design:** [F10.1](F10.1-captured-input-view-design.md) (API, evidence, authorization order, limits, errors). This plan does not repeat it; it fixes the build order, the code seams and the decisions the design left open. Facts below were read in the code at `origin/main` (`e1cdb515`).

## Scope and finish line

Slices F10.2 (tracked contained `read`/`exists`/`list` with limits) and F10.3 (revalidation, reject-on-change). Done when the [F10 acceptance list](F10-captured-input-view.md#acceptance) passes on Linux and Windows and macOS is a tested `Unsupported`.

Recorded maintainer decisions (see [README](README.md#decisions-for-finishing-the-standalone-features)), encoded here:

- **D2:** symlinks are configurable and **rejected by default**. A "follow" mode is a typed option, off by default (see F10.2c).
- **D5:** **macOS returns `Containment::Unsupported`** until a verified beneath-root open exists. No best-effort walk on macOS.
- **D9:** **reject** Windows alternate data streams and reserved device names.

Out of scope: plugin permission schema and hook host services, the restart loop, a transactional snapshot, any change to the task hasher or its index, and edits to `omni_capabilities`, `omni_capability_sys`, `omni_capability_enforcement`, `dir_walker`, `omni_ignore_*` and `omni_glob*`. No discovery consumer is wired here; discovery adopts the view in its own plan (F10 is "usable by", not "used by").

## What the code already gives us

| Observed                                                                                                                                                                                                                                                                                                                                                 | Consequence                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `crates/omni_task_executor/src/cache_restore.rs` already does handle-relative, no-follow filesystem work with `cap_std::fs::Dir` and `cap_fs_ext::DirExt` (workspace deps `cap-std 4.0.3`, `cap-fs-ext`; [tooling](../../../../tooling.md)). It has symlink-swap, parent-swap, junction and retained-handle-cannot-be-renamed tests on Unix and Windows. | Build the view on `cap-std` rather than hand-written `libc`/`rustix`/`windows-sys` (the F10.1 sketch). This changes F10.1's dependency list and is **open question Q1**; it is the first thing the proof fixture settles. Reuse its test shapes. |
| `omni_path_utils::WorkspacePath` rejects empty, NUL, `\`, drive prefix, absolute, `.`/`..` and empty segments. It does **not** reject `name:stream` after the first segment or Windows reserved names. `omni_path_utils::escape_path_component` knows the reserved-name list (`lib.rs`) but is an escaper, not a validator.                              | D9 is a new access-time name check inside the view (F10.2a), not a `WorkspacePath` change (Q3).                                                                                                                                                  |
| `omni_capability_sys::PolicyEnforcingSys::guard` authorizes a symlink-resolved real path, then forwards to `TSys`. Its docs admit the check/open race. `CapabilityAuthorizer` (`authorize.rs`) and `omni_capabilities::evaluate` / `Request::Fs { write, path }` are the policy entry points.                                                            | The view needs `fs.read` decisions on a lexical path. Define `InputAuthorizer` in the new crate; a small adapter over `CapabilityAuthorizer` lives in the **consumer**, not in either capabilities crate, so those crates stay unchanged.        |
| `omni_hasher::Hasher` (`Hasher`, `Blake3Hasher`, `DefaultHasher`); the crate also pulls `system_traits`, `rs_merkle`, `futures`, `eyre`. `omni_path_utils` already depends on `sha2`.                                                                                                                                                                    | Digest trait: prefer a local two-method trait with a `blake3` implementation, to keep `omni_input_view` a light leaf (D4, Q4).                                                                                                                   |
| `omni_hasher::project_dir_hasher::impls::utils` trusts `(mtime ms, size)` equality. `dir_walker` yields entries without capture or containment.                                                                                                                                                                                                          | Neither is touched. The view never reads the hasher index and does not use `dir_walker` (D10: list-only membership evidence).                                                                                                                    |
| `omni_remote_source` (`manager/tree.rs`, `manager/pending.rs`, `driver.rs`) uses `symlink_metadata` and `is_safe_segment` for its own store paths.                                                                                                                                                                                                       | Not a consumer and not changed. Its segment rules are a separate, store-local policy; do not share code.                                                                                                                                         |

## Slices

Each slice leaves a working system, adds a crate nothing depends on yet, and is reviewed separately. Every slice is High risk: second reviewer, platform CI results attached, and no edit to a capabilities crate in the same PR.

### F10.2a: crate skeleton and thin proof fixture

**Risk:** Medium (no I/O of consequence yet), landed first so the dangerous slice is reviewable.

- New leaf crate `crates/omni_input_view` (workspace `crates/*` glob already includes it; add the `[workspace.dependencies]` entry). Dependencies: `omni_path_utils`, `cap-std`, `cap-fs-ext`, `blake3`, `thiserror`. No `system_traits`, plugin, runtime, Wasm or discovery crate.
- Types from F10.1 with no behavior: `InputLimits`, `InputError`, `ContainmentViolation`, `BudgetKind`, `Observation`, `Evidence`, `Cancellation`, `InputAuthorizer`, `ContainmentMode`.
- **Proof fixture (the thin slice):** one function opening a file beneath a `cap_std::fs::Dir` root via `WorkspacePath` segments, with `ContainmentViolation::Symlink` on any symlink component, tested with a symlink-swap loop on Linux and Windows. It answers Q1 (does `cap-std` give a race-safe beneath-root open on the kernels and Windows versions we support, including the kernel without `openat2`?) before the API is built on it. If it does not, re-plan (see triggers).
- Name validation for D9: reject `:` in any segment on Windows builds, and reject reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`, with or without an extension, any case) plus trailing dot or space. Table-driven unit tests run on all OSes (pure function); the I/O behavior runs only on Windows.

**Gate:** the swap test shows no outside bytes in 10,000 iterations on each OS, and the macOS path returns `Unsupported` without reading (D5).
**Rollback:** delete the crate; nothing depends on it.

### F10.2b: tracked `read` / `exists` / `list` with limits

**Risk:** High (containment, authorization). Implements [F10.1 design](F10.1-captured-input-view-design.md#design-proposed): authorize first, then open beneath the root, `fstat` regular-file check with non-blocking open, limits checked before allocation, per-entry `list` authorization with the separate raw-scan counter, evidence kept per observation with conflict marking, cancellation recording nothing.

Interfaces and data shapes are those in F10.1 (`InputView::{open, read, exists, list, evidence}`; `Read`, `Exists`, `Listing`, `Observation`, `PathEvidence`, `InputError`). Decisions fixed here:

- `InputView::open` takes an absolute workspace root and holds one `cap_std::fs::Dir`. No API returns a `Dir`, handle or absolute path.
- Authorization uses the lexical path (root path joined with the validated suffix) because symlinks are rejected in the default mode.
- Default `InputLimits` come from [§8.1](../../008.00-performance-and-shared-safeguards.md#81-shared-safeguards) host limits if present, otherwise the benchmark below (Q5); the checks do not depend on the values.
- `ENOENT` and `ENOTDIR` are `Missing`; `EACCES`, symlink, non-regular and other errors are `InputError` (D6 recommended default, confirm in Q6).

**Gate:** the F10.1 F10.2 test list, plus the benchmark on a 1,000-file tree in `crates/omni_bench` against plain reads. Unchanged existing tests in `omni_capabilities` and `omni_capability_sys` prove authority was not expanded.
**Rollback:** revert the slice; the crate has no consumers.

### F10.2c: configurable symlink policy (D2)

**Risk:** High. `SymlinkPolicy::{Reject (default), FollowInRoot { max_hops }}` on `InputLimits` or `InputView::open`.

- `Reject` is the default and the only mode the proof fixture and slice b tests assume.
- `FollowInRoot`: the walker resolves each link itself, caps hops, re-authorizes every resolved `WorkspacePath`, and rejects any target that leaves the root, including absolute targets and `..` chains. Never `canonicalize` then open. The existing `cache_restore.rs` `Workspace::relative_target` and `resolve_link` are the model, but they are written for a trusted restore, not for reads of untrusted trees, so copy the ideas, not the code.
- Evidence records the requested path, not the resolved one; revalidation re-resolves, so a retargeted link is `Modified`.
- Windows: junctions and mount points stay rejected even in follow mode unless Q2 says otherwise.

**Gate:** escape matrix (absolute target, `..` target, loop, dangling, retarget mid-read) on Linux and Windows, with the default mode re-run to prove it is unchanged. Ship the follow mode only if the maintainers still want it after F10.2b (Q2); the slice may be dropped without affecting the rest.
**Rollback:** remove the option; default behavior is already `Reject`.

### F10.3: revalidation and reject-on-change

**Risk:** High (decides whether stale input is accepted). `InputView::revalidate`, `Revalidation`, `Change`, `ChangeKind` per F10.1. Always re-read and re-digest tracked files; `FileIdentity` is advisory only and never skips the digest. Denial, containment or I/O failure during revalidation is an error, not a `Change`. Output truncates at `max_reported_changes`. Default consumer policy "reject on any `Changed`" is a helper, with no retry loop.

**Gate:** same-size same-mtime edit (`set_modified` restore), `Created` after `Missing`, `Deleted`, `KindChanged`, `MembershipChanged`, atomic replace with identical bytes is `Unchanged`, repeatability, cancellation leaves state untouched.
**Rollback:** revert the slice; F10.2 evidence remains usable without revalidation.

## Test matrix

| Case                                                               | Linux                                                                         | Windows                                                                                  | macOS                         |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ----------------------------- |
| Path/name validation (pure)                                        | run                                                                           | run                                                                                      | run                           |
| Containment: symlink final, intermediate, dangling                 | run                                                                           | run (symlink and junction; skip when symlink privilege is missing, with a named skip)    | assert `Unsupported`, no read |
| Swap-race loop, rename out and back                                | run (`openat2` path; fallback path returns `Unsupported` unless `BestEffort`) | run (held handles, no `FILE_SHARE_DELETE`; if rename succeeds, Windows is `Unsupported`) | none                          |
| Windows ADS and reserved names (D9)                                | name table only                                                               | run with real files                                                                      | name table only               |
| Limits before allocation, FIFO/device (`NotRegular`), cancellation | run                                                                           | limits and cancellation; FIFO n/a                                                        | `Unsupported`                 |
| Revalidation matrix                                                | run                                                                           | run                                                                                      | `Unsupported`                 |
| Benchmark, 1,000 files                                             | run                                                                           | optional                                                                                 | n/a                           |

macOS has no CI target in `ci.yaml` today except release builds (`manual-release-omni.yaml`): confirm what CI runs before claiming macOS coverage (Q7).

## Compatibility and release notes

- No config schema, wire shape, cache key or on-disk format changes; the crate has no consumers, so there is **no user-visible change** in F10.2-F10.3. No release note for the crate itself.
- Add to the release notes only when a consumer lands: new rejection errors (`Containment`, `Denied`, budget errors) and the D2/D5/D9 behavior, as documented limits ("macOS unsupported", "symlinks rejected unless configured", "Windows streams and reserved names rejected").
- Update `docs/tooling.md` if `cap-std` is used from a second crate (it currently names only the executor's `cache_restore` module), and add `omni_input_view` to the check list in the same PR. Update the [README](README.md) F10 row and `tasks.yaml` status in each slice PR, not before.

## Re-planning triggers

Stop and re-plan if any of these happens:

- The proof fixture shows `cap-std` is not race-safe on a supported kernel or Windows version (then raw `openat2`/handle code, a security-review-heavy path, or `Unsupported` for that platform).
- Windows rejects the held-handle rename boundary: Windows becomes `Unsupported` like macOS, and the acceptance wording changes.
- The authorizer adapter cannot express a per-entry `list` decision without changing `omni_capabilities` (that crate is High risk and not in scope).
- A consumer needs `Path` or handles from the view, or recursion beyond one level per call.
- Default limits from the benchmark make a realistic discovery pass exceed budget.
- A change is needed in `dir_walker`, glob or ignore crates (High risk; separate plan).

## Open questions for the maintainer

- **Q1:** Build on `cap-std` (already used by `cache_restore`) instead of raw `libc`/`windows-sys` as F10.1 sketches? Recommended yes, subject to the proof fixture.
- **Q2:** Is `FollowInRoot` (F10.2c) wanted in the first release, or is "configurable" satisfied by a reserved, unimplemented option? Should Windows junctions ever be followed?
- **Q3:** Put the D9 name check in the view only (recommended) or also in `WorkspacePath::parse` (touches a shared type used elsewhere; ADS/reserved names are valid on Unix)?
- **Q4:** Local digest trait with `blake3` (recommended) or reuse `omni_hasher::Hasher` (pulls `system_traits` and async deps)?
- **Q5:** Default `InputLimits` values, and whether §8.1 already defines host limits to reuse.
- **Q6:** Confirm D6 (`ENOTDIR` on a parent is `Missing`), D8 (case-spelling mismatch: error or record on-disk spelling) and D3 (identical-bytes atomic replace is not a change).
- **Q7:** Does CI run native macOS and Windows Rust tests for this crate (the `omni_setup` preset runs Windows tests; confirm the preset for a new crate), and who verifies macOS for a future beneath-root open?
- **Q8:** Is `BestEffort` containment on the Linux fallback allowed at all, or should the fallback always be `Unsupported`?
