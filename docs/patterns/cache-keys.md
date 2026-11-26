# Cache keys

A task's cache key is a hash of its inputs (`omni_collector::Collector::get_digest`).
Local entries, remote artifacts, the output directory and stale pruning all address by
it. Keys are **not a store format** and are never migrated.

## Key-change policy

A change to what a key is computed from (a new, removed or re-encoded input, a different
leaf order, a different input collection rule) is a **key change**. A PR that makes one:

1. **Bumps a version tag.** Add a named, versioned leaf (the identity leaf
   `omni.task-id\0\x01` and the effective-task leaf `omni.effective-task\0\x01` are the
   model) or bump the version byte of the leaf you changed. An encoding owned by another
   module (`effective_task::VERSION`) bumps its own tag too. Never change an unversioned
   leaf's bytes silently; wrap the new behavior in a versioned leaf.
2. **Is a cold start, never a migration.** Old entries become unreachable; they are not
   rewritten, promoted, dual-read or probed for under the old key. A fallback read is safe
   only for a pure re-encoding of _identical_ inputs. A new input is never that: an old
   entry did not record it, so promoting it could return a wrong hit.
3. **Ships a release note** (`apps/site/content/docs/guides`) with the cold start:
   what changes, that old entries are not matched, `omni cache prune --stale` once the
   fleet is aligned, how to see it, and rollback.
4. **Batches with other key changes in the same release.** Before adding one, list the
   key changes already unreleased since the last tag (`git log <last-tag>..HEAD --
   crates/omni_collector crates/omni_hasher crates/omni_task_context
   crates/omni_cache/src/cache/impls/hybrid.rs`) and land it in the same release if
   one is pending, so users get one cold start. Say in the PR what the epoch holds.
5. **Moves the pinned vectors**: the collector's golden keys and the e2e hash goldens
   change in the same commit, and the commit is `feat!`/`fix!` so the release notes
   catch it.
6. **Changes every constructor at once.** Every place that builds the inputs (collector
   callers, the dry run, the hash commands, the hybrid store and stale pruning) takes
   the input as a non-optional field so the compiler finds them; a key change must not
   leave two recipes in one binary.

Platform, the invocation and display data are **not** key inputs unless a decision says
so; do not add them to make a test pass.

## Rehearsing mixed versions

Rollback is by binary, so rehearse both directions against one disposable store:

1. Build the previous binary from the base SHA in a temporary `git worktree`.
2. Set `OMNI_TEST_BASELINE_BIN` to it and `OMNI_TEST_BIN` to the new one, then run
   `tests/omni-tests/src/cache-cross-version.e2e.spec.ts`. After a key change it must
   assert that neither side hits the other's entry and that each side still hits its
   own; update its expectations in the same PR.
3. For a remote, use the fake remote in `omni_cache` tests: the new binary must make no
   request under the old key.
4. Check `omni cache prune --stale --dry-run` from each binary: it classifies the other
   version's entries as stale. That is expected; do not prune while both are in use.

See [F06.3](../rfc/0024-tiered-project-discovery-plugins/implementation/features/F06.3-plumbing-plan.md)
for the first use of this policy and
[task-identity.md](task-identity.md) for the identity leaf.
