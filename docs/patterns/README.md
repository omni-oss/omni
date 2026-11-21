# Patterns

Keeping this up to date: when a pattern changes, update its example paths and this index in the same PR. Remove entries the code no longer follows.

- [errors.md](errors.md): error types and diagnostics in Rust crates.
- [diagnostic-prefixes.md](diagnostic-prefixes.md): error and notice code prefix ownership.
- [source-kinds.md](source-kinds.md): adding a remote-source kind (`SourceDriver`) and publishing staged reference updates.
- [scaffolding.md](scaffolding.md): creating crates and packages from generators.
- Cache output restoration: `crates/omni_task_executor/src/cache_restore.rs`
  confines writes with retained directory capabilities, no-follow traversal and
  exclusive creation. Do not replace this with pathname checks followed by
  `std::fs` writes.
