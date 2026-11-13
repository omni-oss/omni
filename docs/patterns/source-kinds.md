# Remote-source kinds

A remote-source *kind* (`git`) is the id string used as a store path segment and a lockfile key. Its *driver* implements `SourceDriver` (`crates/omni_remote_source/src/driver.rs`): `kind()`, `slug()`, `resolve()` and `export()`. The store core (`crates/omni_remote_source/src/manager/default_impl.rs`) owns locks, publishing, manifests, pins and prune, and must not mention any kind; a unit test checks it does not import git.

Example driver: `crates/omni_remote_source/src/git.rs`. Test-only driver exercised through the core: `crates/omni_remote_source/tests/fake_kind.rs`.

To add a kind:

1. Implement `SourceDriver`. `resolve` returns a pin that is a safe segment (`[a-z0-9._-]`, at most 128 bytes, no leading `.`); with a locked pin it must return that pin. `export` writes only into `dest` and the driver's `raw_dir`. Connect with `spec.connection`, never `spec.locator`, and keep credentials out of the locator.
2. Register it in `DriverRegistry::builtin`.
3. Add a `RemoteSource` variant and its arms in `kind()` and `spec()` (`crates/omni_remote_source/src/source.rs`).
4. Map its `SourceConfig` variant in `remote_source_for` (`crates/omni_remote_source_contributors/src/lib.rs`).

The lockfile, refs, prune and projection code need no change.
