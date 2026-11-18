# RFC 0030: independent-review follow-up

## Assessment and approval

Large, High-risk work: cache persistence, trust policy, concurrent invalidation, and
cross-platform filesystem writes. Several components change; the largest unknown is
race-resistant Windows restore. Source changes can be reverted, but weaker security
must not be used as a fallback. One implementation owner; independent review before
commit. The user approved strict verification, race-resistant restore, and the
remaining review fixes. No production migration or unrelated refactor is included.

## Decisions

- `require` never consumes unsigned remote entries, including isolated entries.
  Do not let probe scopes weaken it. Keep unsigned isolated reuse for `off`/`warn`.
- Replace pathname-based replay with directory-capability operations using
  `cap-std`/`cap-fs-ext` rather than maintaining raw Unix and Windows syscalls.
  Open each parent without following links; retain handles; exclusively create
  destination files. Never truncate a pre-existing symlink or hardlink. Unsafe
  restore must not count as a successful cache hit.
- Keep 200,000 entries and 8 GiB expanded content, but also bound raw decompression
  and extension metadata before the tar parser can allocate it.
- Validate remote metadata before publication. Reject and safely replace invalid
  persisted entries under existing cache locks. Test rebuild/recache, not just miss.
- Transition LRU purge generation and clear under one mutex.
- Accept publish-only setup. Make unknown partitions per-store/per-run rather than
  claiming a reusable directory is private. Correct final migration documentation.

## Steps, checks, and progress

1. [x] Implement directory-capability replay, including parent swaps and final
   symlink/hardlink regressions (Linux pass; native Windows validation pending CI).
2. [x] Strict verification against a malicious probe and isolated-origin local
   markers. Test unsigned rejection plus successful signed shared fallback.
3. [x] Archive raw-byte/extension/effective-size bounds. Test PAX-size overrides,
   extension bombs, boundary counts, normal GNU long paths and archive round trips.
4. [x] Invalid-entry recovery. Test invalid decode/identity/path, rebuild, recache,
   lookup, stats and prune; serialize replacement with existing locks.
5. [x] LRU generation/clear atomicity with deterministic lock-boundary regression
   and existing in-flight prefix-delete integration test.
6. [x] Publish-only setup, unknown-partition behavior, and documentation.
7. [x] Combined tests, fmt, clippy, diff review, independent Opus 5.5 review
   (no blockers; native Windows validation still pending).

Happy path: a signed shared result restores inside the workspace. Hard cases:
a malicious server lies about grants, a tar extension overrides size, a poisoned
entry is rebuilt, a directory is swapped for a symlink during replay, and a streamed
writer finishes during purge. None may bypass verification, exhaust unbounded
resources, produce an outside write, or report a missing output as a valid hit.

Run targeted crate tests while implementing; run the combined cache, executor,
setup, storage, and configuration checks once at integration. Leave native Windows
runtime checks to CI and state that limit. Never weaken tests.

Undo each source slice by reverting its dedicated commit. Do not revert the
existing PR or touch `.omni/sources/lock.json`. Commit/push only within the user's
existing PR authorization, after independent review; no history rewrite needed.

## Implementation notes and evidence

- `cap-std`/`cap-fs-ext` 4.0.3 were chosen for maintained, cross-platform directory
  capabilities and no-follow directory opens, rather than custom unsafe syscall
  code. Their APIs and Windows directory share flags were inspected locally.
  Files are copied into exclusive fresh files; existing regular outputs remain
  untouched. Open handles are bounded by path depth rather than output count.
- The workspace root and local cache remain trusted. Directory-relative writes
  resist path-to-symlink swaps; this is not process isolation or a sandbox.
- Recovery uses the existing per-partition prune/replacement locks. Local entry
  reads/repairs and publishers now serialize against each other within a partition,
  including during remote retrieval; downloads within one request still use eight
  workers. This favors a simple, coordinated repair protocol over cross-process
  parallel publication. Per-entry locking is a possible future optimization.
- Archive hardening was delegated as a bounded, single-file slice. Parent inspected
  the result, added mixed PAX-size/extension compatibility coverage, and ran combined checks.
  Raw overhead counts toward 8 GiB; validation/extraction uses three passes.
- Combined `cargo test` passed (491 tests) for cache, executor, storage core, memory backend,
  setup and configurations. Scoped fmt and core/executor test clippy pass; setup
  production clippy passes. Setup test clippy remains blocked by its pre-existing
  `println!` in `fallback_store/tests.rs:610`, not changed here.
- No live cloud/database or native Windows tests were rerun in this follow-up.
- Opus reviewed the uncommitted follow-up and ran executor/archive tests: no
  blockers. Its smaller findings were addressed with a junction regression,
  raw-component traversal for `link/../out`, corrected PAX documentation/coverage,
  and explicit writable-cache/symlink/unknown-directory maintenance notes.
