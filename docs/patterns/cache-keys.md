# Cache keys

A task's cache key is a hash of its inputs (`omni_collector::Collector::get_digest`).
Local entries, remote artifacts, the output directory and stale pruning all address by
it. Keys are **not a store format** and are never migrated.

## Key-change policy

A change to what a key is computed from (a new, removed or re-encoded input, a different
leaf order, a different input collection rule) is a **key change**. A PR that makes one:

1. **Bumps a version tag.** Add a named, versioned leaf (today the identity leaf
   `omni.task-id\0\x01` is the model) or bump the version byte of the leaf you changed.
   An encoding owned by another module (`effective_task::VERSION`) bumps its own tag
   too. Never change an unversioned leaf's bytes silently; wrap the new behavior in a
   versioned leaf.

    **Cache key version tag (lands with the F06.3 cutover, slice 2; not in the code yet).**
    The cutover adds one constant, `CACHE_KEY_VERSION`, in `omni_collector`, and a leaf
    `BLAKE3(b"omni.cache-key\0" || u32_le(CACHE_KEY_VERSION))` inserted after the
    identity leaf. From then on rule 1 is enforceable: **bump the cache key version (`CACHE_KEY_VERSION`) when any key input or the key
    construction changes**, besides the leaf you touch, and the
    golden vectors move with it. Until the cutover lands, only the identity leaf's own
    byte is a version; do not cite the cache key version leaf as existing.

2. **Is a cold start, never a migration.** Old entries become unreachable; they are not
   rewritten, promoted, dual-read or probed for under the old key. A fallback read is safe
   only for a pure re-encoding of _identical_ inputs. A new input is never that: an old
   entry did not record it, so promoting it could return a wrong hit.
3. **Ships a release note** (`apps/site/content/docs/guides`) with the cold start:
   what changes, that old entries are not matched, `omni cache prune --stale` once the
   fleet is aligned, how to see it, and rollback.
4. **Batches with other key changes in the same release.** Before adding one, list the
   key changes already unreleased since the last tag and land it in the same release if
   one is pending, so users get one cold start. Say in the PR what the epoch holds. The
   audit must cover every place a key input is built, not only the collector:

    ```
    git log --oneline <last-tag>..HEAD -- \
      crates/omni_collector crates/omni_hasher crates/omni_task_context \
      crates/omni_cache/src/cache/impls/hybrid.rs crates/omni_cache/src/exts.rs \
      crates/omni_context/src/project_data_extractor.rs \
      crates/omni_context/src/project_hasher.rs \
      crates/omni_context/src/workspace_hasher.rs \
      crates/omni_context/src/loaded_context.rs crates/omni_context/src/env_loader \
      crates/omni_glob_config crates/dir_walker \
      crates/omni_task_executor/src/cache_manager.rs \
      crates/omni_task_executor/src/batch_executor.rs \
      crates/omni_core/src/task_execution_graph.rs
    ```

    Read the diffs, not the subjects: aggregate digest forwarding, glob sides and the
    task graph changed keys without saying so. Say honestly how many cold starts the
    release holds; the list is a best effort, not a proof.

5. **Moves the pinned vectors**: the collector's golden keys and the e2e hash goldens
   change in the same commit, and the commit is `feat!`/`fix!` so the release notes
   catch it.
6. **Changes every constructor at once.** Every place that builds the inputs (collector
   callers, the dry run, the hash commands, the hybrid store and stale pruning) takes
   the input as a non-optional field so the compiler finds them; a key change must not
   leave two cache key versions in one binary.

Platform and display data are **not** key inputs unless a decision says so; do not add
them to make a test pass. The **merged invocation args** (`omni run t --arg k=v`) **are**
key inputs (F06.3 decision Q1, through `TaskExecutionInfo.args`; the effective digest
itself stays invocation-free); until the cutover lands they are not, which is a known
wrong hit. The loaded environment (`.env` contents, process environment) is not keyed;
users opt in with `cache.key.env` and `cache.key.files`.

## Rehearsing mixed versions

Rollback is by binary, so rehearse both directions against one disposable store:

1. Build the previous binary from the base SHA in a temporary `git worktree`.
2. Set `OMNI_TEST_BASELINE_BIN` to it and `OMNI_TEST_BIN` to the new one, then run
   `tests/omni-tests/src/cache-cross-version.e2e.spec.ts`. Across a key change set
   `OMNI_TEST_EXPECT_COLD_START=1` (phase 2 of F06.3 adds the switch): neither side hits
   the other's entry; by default it asserts sharing. Either way each side must hit its
   **own** entry. Flip the switch off again for the first release whose baseline is
   after the key change.
3. For a remote, use the fake remote in `omni_cache` tests: the new binary must make no
   request under the old key.
4. Check `omni cache prune --stale --dry-run` from each binary: it classifies the other
   version's entries as stale. That is expected; do not prune while both are in use.

See [F06.3](../rfc/0024-tiered-project-discovery-plugins/implementation/features/F06.3-plumbing-plan.md)
for the first use of this policy and
[task-identity.md](task-identity.md) for the identity leaf.
