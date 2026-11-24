# RFC 0032 slice 1 findings

**Status:** Draft for independent review. This records what the slice 1 harness ([#170](https://github.com/omni-oss/omni/pull/170), `crates/omni_migration_proof`) showed about [RFC 0032](0032-pre-v1-migration-infrastructure.md). It gates slice 2 and does not authorize it.
**Readers:** The maintainer deciding whether to extract `omni_migration`, and the reviewer checking that the evidence supports that decision.

## Question

The RFC's open design question: can a shared planner and runner serve a store with atomic publication (the lockfile fixture) and one without (the index fixture), with truthful commit outcomes and held coordination, and **without storage-specific branches in the runner**? The RFC says to stop and revise if either fixture needs storage-specific orchestration, misleading outcomes or a different guard ownership.

## Answer

Supported for these two fixtures, on one local backend (Linux, process kill and simulated persistence failures). The runner (`src/core.rs`, about 430 lines including the planner) names no storage type, path, file or format. Both adapters implement the same two traits and share one `execute`. The harness also exposed six places where the RFC's contract was underspecified. Each needed a change or a decision before extraction.

## Evidence

38 tests at merge (46 after review fixes) run the RFC's acceptance cases against the real fixture protocols with failpoints, plus subprocess kill and a barrier race. A case counts as covered when a test fails if the behavior is removed; the first reviewer re-verified that by reverting a fix.

| RFC case                        | Result                                                                                                                                                                                                                                                                                       |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lockfile round trip and refusal | Covered. Meaningful-note refusal is `NotCommitted` with bytes unchanged and no writes.                                                                                                                                                                                                       |
| Chain identity binding          | Covered for both adapters: missing, malformed and foreign names; edited step ID and edited version are `StaleCatalog`; appending a transition stays valid.                                                                                                                                   |
| Intent chain name               | Covered for both adapters.                                                                                                                                                                                                                                                                   |
| Backend capability gate         | Covered for the lockfile at plan, preflight and in-step. For the index only the plan-time gates are covered.                                                                                                                                                                                 |
| Full plan before writes         | Covered, including `NoDowngradePath` against `MissingStep` and invalid chain shapes.                                                                                                                                                                                                         |
| Policy is directional           | Covered through `plan` and `execute`, including an irreversible second leg.                                                                                                                                                                                                                  |
| Guarded stale check             | Covered: peer edit, same-data new generation, replaced store, peer reaching the target, capability change.                                                                                                                                                                                   |
| Lockfile interruption           | Covered by failpoints at stage, intent rename, intent parent sync, document rename, parent sync and intent removal, plus subprocess kill after intent and replace. False `Committed` after an ambiguous write is exercised for the lockfile and for the index marker (`marker.parent_sync`). |
| Non-atomic interruption         | Covered: kill after first shard removal, dirty despite matching marker, byte-for-byte rollback, retry of an interrupted recovery, resurrected intent.                                                                                                                                        |
| Prefix commit                   | Covered for `Stopped` and `NeedsRecovery`.                                                                                                                                                                                                                                                   |
| Cancellation and concurrency    | Partly: cooperative cancellation, guard exclusion while a writer is live, and the survivor rejecting its stale plan. No async drop-awaiter case.                                                                                                                                             |
| Zero-work path                  | Partly: `UpToDate` and `Dirty` with a panicking cold closure. No panic-on-access registry or session fakes, no allocation count, no benchmark.                                                                                                                                               |

## Where the contract needed more than the RFC said (slice 1 review: PASS for 5eb38f0f, wording refined after)

1. **Pending-intent refusal belongs below `execute`.** The RFC has `execute` re-observe under the guard, which refuses a pending intent. A caller that holds a session and calls `apply` directly skips that, and the first `apply` overwrote the unresolved intent. Both adapters needed the same check. Extraction should either make `apply` unreachable except through `execute` (sealed session) or put the check in the shared runner before every step.
2. **Recovery must validate the evidence, not just match it.** An intent that matched the old marker but carried a rollback marker for a different version would have been "recovered" into an unreadable store. Recovery now checks that the rollback keeps the old version and that target and rollback revisions are old+1 and old+2. The RFC's recovery rules should state this.
3. **Unguarded observation needs a snapshot rule.** The RFC says the caller must obtain a coherent snapshot. A single read is fine for the one-file lockfile. For the multi-file index a reader can pair a marker and records that never coexisted. The harness re-reads (lockfile: re-check the intent after the document; index: retry until marker, records and intent agree). A guarded read was rejected because observation must still report a live writer's intent. Anything torn is rejected as stale or dirty by the guarded re-observation. This is adapter code, which is acceptable only because it is behind `observe`. Slice 2 should record it as an adapter obligation.
4. **Wrong version at step time maps differently in the two adapters.** The lockfile returns `RecoveryRequired(UnexpectedVersion)` and the index returns `NotCommitted(UnexpectedVersion)`. Only the index case is tested. A readable, healthy document at another version is a proven old state, so `NotCommitted` is the accurate outcome. The contract should state the mapping for pre-write failures as an adapter obligation, and the lockfile should be aligned.
5. **The runner does not re-observe between steps.** It relies on each `apply` re-checking `s.from`; nothing between steps checks store ID or revision. The guard makes that sufficient here, but it is an adapter obligation alongside item 1.
6. **Arithmetic and creation edges.** Generation and revision overflow, a body that does not match its version, and an unsupported index version all produced success or panics before review. They are adapter validation, not runner logic.

None of these needed a storage-specific branch in the runner. They did require adapter obligations and one sealing decision (item 1).

## What is not evidenced

- Power loss, network or shared filesystems, and any platform but Linux. Process kill does not simulate power loss.
- Async execution and dropping an awaiter while a detached writer runs.
- Zero-work cost: no allocation or event counts, no benchmark against bare `Version` equality (slice 3).
- The index half of the unsynced intent removal case (only the lockfile has it).
- Blocking guard acquisition: the live-writer test uses `try_lock_exclusive`, so the `lock()` path used by `begin_migration` is untested and `GuardBusy` is unreachable through `execute`.
- Sync-level failpoints inside file and shard syncs, an index kill during a rollback run, and index backend flips at preflight and in-step.
- A blocked second `execute` racing a live writer. Only guard exclusion and the post-release stale rejection are checked.
- The chain digest uses `DefaultHasher`, which is not stable across Rust versions. It is fine in-process only.
- `Plan` fields are public in the harness, which the RFC forbids.

## Carry-over for slice 2

- Seal `Plan` construction and decide item 1 (sealed session or shared pre-step check).
- Replace the chain digest with a stable hash.
- Write items 2 to 5 into the contract as adapter obligations, and align the lockfile's wrong-version outcome with the index.
- Close the zero-work fakes and the index backend-flip cases, or record them as slice 3 work.

## Recommendation

Proceed to slice 2 only after an independent reviewer agrees that the evidence above supports extraction and that the carry-over list is complete, and after the maintainer approves. If the reviewer finds a case where the runner needed a storage-specific branch, revise the RFC instead.
