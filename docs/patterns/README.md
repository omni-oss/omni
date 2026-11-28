# Patterns

Keeping this up to date: when a pattern changes, update its example paths and this index in the same PR. Remove entries the code no longer follows.

- [sys-abstraction.md](sys-abstraction.md): route filesystem calls through `*Sys` / `system_traits` handles instead of `std::fs` or `tokio::fs`.
- [errors.md](errors.md): error types and diagnostics in Rust crates.
- [diagnostic-prefixes.md](diagnostic-prefixes.md): error and notice code prefix ownership.
- [source-kinds.md](source-kinds.md): adding a remote-source kind (`SourceDriver`) and publishing staged reference updates.
- [scaffolding.md](scaffolding.md): creating crates and packages from generators.
- [task-identity.md](task-identity.md): structured `(project, task)` identity, escaped display labels and the cache-key identity leaf.
- [cache-keys.md](cache-keys.md): the key-change policy (any key input change bumps `CACHE_KEY_VERSION` and ships a release note; cold start, no migration, batching) and rehearsing mixed versions with `OMNI_TEST_BASELINE_BIN` and `OMNI_TEST_EXPECT_COLD_START`.
- Cache output restoration: `crates/omni_task_executor/src/cache_restore.rs`
  confines writes with retained directory capabilities, no-follow traversal and
  exclusive creation. Do not replace this with pathname checks followed by
  `std::fs` writes.
