# Remote-source kinds

A remote-source _kind_ (`git`) is the id string used as a store path segment and a lockfile key. Its _driver_ implements `SourceDriver` (`crates/omni_remote_source/src/driver.rs`): `kind()`, `slug()`, `resolve()` and `export()`. The store core (`crates/omni_remote_source/src/manager/default_impl.rs`) owns locks, publishing, manifests, pins and prune, and must not mention any kind; a unit test checks it does not import git.

Example driver: `crates/omni_remote_source/src/git.rs`. Test-only driver exercised through the core: `crates/omni_remote_source/tests/fake_kind.rs`.

To add a kind:

1. Implement `SourceDriver`. `resolve` returns a pin that is a safe segment (`[a-z0-9._-]`, at most 128 bytes, no leading `.`); with a locked pin it must return that pin. `export` writes only into `dest` and the driver's `raw_dir`. Connect with `spec.connection`, never `spec.locator`, and keep credentials out of the locator.
2. Register it in `DriverRegistry::builtin`.
3. Add a `RemoteSource` variant and its arms in `kind()` and `spec()` (`crates/omni_remote_source/src/source.rs`).
4. Map its `SourceConfig` variant in `remote_source_for` (`crates/omni_remote_source_contributors/src/lib.rs`).

The lockfile, refs, prune and projection code need no change.

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
