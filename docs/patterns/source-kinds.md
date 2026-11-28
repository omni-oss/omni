# Remote-source kinds

A remote-source _kind_ (`git`) is the id string used as a store path segment and a lockfile key. Its _driver_ implements `SourceDriver` (`crates/omni_remote_source/src/driver.rs`): `kind()`, `slug()`, `resolve()` and `export()`. The store core (`crates/omni_remote_source/src/manager/default_impl.rs`) owns locks, publishing, manifests, pins and prune, and must not mention any kind; a unit test checks it does not import git.

Example driver: `crates/omni_remote_source/src/git.rs`. Test-only driver exercised through the core: `crates/omni_remote_source/tests/fake_kind.rs`.

To add a kind:

1. Implement `SourceDriver`. `resolve` returns a pin that is a safe segment (`[a-z0-9._-]`, at most 128 bytes, no leading `.`); with a locked pin it must return that pin. `export` writes only into `dest` and the driver's `raw_dir`. Connect with `spec.connection`, never `spec.locator`, and keep credentials out of the locator.
2. Register it in `DriverRegistry::builtin`.
3. Add a `RemoteSource` variant and its arms in `kind()` and `spec()` (`crates/omni_remote_source/src/source.rs`).
4. Map its `SourceConfig` variant in `remote_source_for` (`crates/omni_remote_source_contributors/src/lib.rs`).

The lockfile, refs and projection code need no change. Prune needs no kind-specific
code either, but it does rely on `slug`: it reads other workspaces' reference files
and maps each `(kind, locator, selector, pin)` back to the store directory with the
pruner's own driver. So `slug` must be a pure, panic-free function of
`(locator, selector)` that never reads `connection` (which is not stored in a
reference); changing it between omni versions silently stops matching old
references.

## Reference updates

Use `RemoteSourceManager::update_refs` with `RefUpdate::ReplaceComplete` only
after resolving the subsystem's entire set; use `MergeAdditive` for genuinely
partial work. An empty complete update releases old refs. The compatibility
`record_refs` wrapper means complete replacement. Corrupt/unreadable existing
sets are errors in either mode, never an excuse to discard old references.

Reference updates share the workspace advisory lock with pin reconciliation.
Refs and lockfiles publish with `omni_utils::atomic_file::write`: exclusive-create
staging in the destination directory, flush/sync, close, replacement rename.
Reference publishers own the advisory guard through commit, including after
async cancellation. For pin persistence/reconciliation, an owned task keeps the
guard through disk publication **and** saved-change/memory cleanup, so cancelling
the waiter cannot leave a retained manager trusting a pruned pin or replaying old
saved changes. Keep staging files non-JSON so reconciliation ignores them.
This guarantees complete-file visibility, not power-loss durability or a
cross-file install transaction.

Install resolves every selected contributor before updating any refs. Its
`update_ref_sets` batch stages all selected files before the first rename under
one workspace lock. A contributor, validation or staging failure changes no ref
set or lockfile. Once renaming starts, a failure can leave a prefix committed;
each file stays complete and re-running install converges. Unselected sets are
not rewritten. `atomic_file::write_batch` implements the staging boundary; it is
not a cross-file rollback mechanism.

## Leases and prune

`RemoteSourceManager::materialize_leased` returns the checkout plus an
`Arc<SourceLease>`; hold it for as long as the tree may be read. `prune_store_report`
skips leased checkouts (`skipped_in_use`), reclaims the others, and reports removal
failures instead of hiding them; `prune_store` returns only what was removed.
Plain `materialize` takes no lease. Lock order is remote lock, then lease, and the
workspace advisory lock is never held while waiting on either. Leases bind
cooperating omni processes only. Design: `docs/rfc/0024-tiered-project-discovery-plugins/implementation/features/F08.1-lease-gc-reference-protocol.md`.

An operation that reads checkouts (generator or tool run, projection sync) must
hold a `LeaseSet` for its whole duration: `manager.materialize_into(&source, &leases)`
for each source, `leases.absorb(&expanded.leases)` for pack roots, and the set bound
to a named variable. Never `let _ = ...`. Do not call plain `materialize` in these
paths; a test in `omni_api` enforces it.

`update_ref_sets` also publishes the workspace's registration
(`<store>/workspaces/<hash>.json`, see `omni_remote_source::registration`) as the
first file of the same `write_batch`, so a crash cannot leave references without a
registration. It is skipped, with the references still written, when the store has
no `VERSION` marker or is a newer layout. Prune reads registrations and never
deletes them (`protection.rs`); the follow-up `workspaces list/forget` command does
not exist yet.
