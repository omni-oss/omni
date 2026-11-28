# Filesystem and process calls (`*Sys`)

Production code that already has a system handle must call through it, not
through `std::fs`, `tokio::fs` or `std::process` directly. A handle is a
crate-owned `*Sys` supertrait (for example `GitUtilsSys`, `GeneratorSys`,
`ApplierSys`) or a set of `system_traits` traits taken as bounds
(`impl FsRead + FsWrite`).

## Where the handle can be

Route the call when a handle is reachable as any of:

- a parameter: `async fn fetch_bare(sys: &impl GitUtilsSys, ...)`
- a field: `self.sys` (`crates/omni_cache/src/cache/impls/hybrid.rs`)
- a context accessor: `self.context.sys()`
  (`crates/omni_task_executor/src/pipeline.rs:93`)

If no handle is reachable, adding one is a separate change with its own review;
do not smuggle a new `Sys` parameter into an unrelated change.

## Preference order

1. **Crate `*Sys` supertrait.** Preferred for crates with a public boundary:
   `GitUtilsSys` (`crates/omni_git_utils/src/sys.rs`), `GeneratorSys`
   (`crates/omni_generator/src/sys.rs`), `ApplierSys`
   (`crates/omni_projections/src/apply.rs`), `RemoteSourceSys`.
2. **Raw `system_traits` traits as bounds.** For internal helpers or one-off
   operations: `sys: &(impl FsOpen + FsRename + FsRemoveFile)` in
   `crates/omni_utils/src/atomic_file.rs`. Add the trait to the bound at the
   nearest site that owns the generic rather than widening a shared supertrait
   for one caller (`FsCanonicalize` on `LazyToolRunner`,
   `crates/omni_tool/src/bridge_runner.rs`).
3. **A helper that calls `std::fs` directly.** Only when `system_traits` has no
   operation for it. Keep the raw call in one place and say why next to it:

   ```rust
   // `system_traits` has no *async* chmod operation, so this stays a raw
   // call. It is best effort: a failure leaves the file read-only.
   let _ = std::fs::set_permissions(path, permissions);
   ```

Prefer the async trait when the caller is async. Prefer the sync trait when the
call is already inside a sync seam (`PathRoots::map_bases`) or when only the
sync form is in the caller's bounds.

## Adding a missing operation

When a call has no trait method, decide before writing code:

- The operation is missing from the crate's `*Sys`: add it to the supertrait and
  forward it in each implementation (`crates/omni_remote_source/src/sys.rs` and
  its test systems forward `FsCanonicalizeAsync`, #202). Widen any `TSys` bound
  that must now prove it.
- The operation is missing from `system_traits` entirely: keep the raw call with
  the comment above. Do not add a `std::fs` helper beside a handle that could
  have done the work.

Known gaps, as of this writing:

- No async streaming open (`File::open` / `OpenOptions`); the sync `FsOpen`
  exists. Log replay and output capture keep raw `tokio::fs` calls.
- No async chmod / permissions; `FsSetPermissions` and `FsFileSetPermissions`
  are sync and take a Unix `mode`, so portable read-only-bit handling stays raw.
- No async advisory file locking; `FsFileLock` is sync.

## Symlink semantics are not uniform

`system_traits` mirrors the two shapes of the standard library:

- `fs_symlink_metadata_async` / `fs_exists_async` / `fs_is_symlink_*` do **not**
  follow a link. `fs_exists_async` reports a dangling link as present.
- `fs_metadata_async` / `fs_is_dir_*` / `fs_is_file_*` follow it.
- `tokio::fs::DirEntry::file_type` and `DirEntry::metadata` never follow a link.

Keep the original behaviour when swapping a `DirEntry` walk for
`fs_read_dir_async`: use `fs_symlink_metadata_async(...).file_type()`.
`tokio::fs::try_exists` follows links and reports a dangling link as absent, so
`fs_exists_async` is not a drop-in replacement for it.

## Tests

Generic code should take a handle so tests can pass `InMemorySys`. A concrete
field such as `self.sys: RealSys` (`crates/omni_cache/src/cache/impls/hybrid.rs`)
still routes through the traits: it keeps the code uniform. Making the store
generic later means threading a type parameter through the struct and its
impls, not a one-line change.

## Examples in tree

- `crates/omni_git_utils/src/lib.rs` (`fetch_bare`): create, remove, rename and
  exists through `GitUtilsSys`.
- `crates/bridge_rpc_runner/src/vendor.rs`: `VendorSys` is a small supertrait for
  a low-level crate that owns no sys; callers pass theirs. `omni_generator` must
  pass `TransactionSys::real_sys()`, not the overlay: a spawned child process
  reads the real filesystem, so buffered overlay writes are invisible to it.
- `crates/omni_cache/src/cache/impls/hybrid.rs`: metadata, read, write, read
  directory and remove through `self.sys`, including the entry-validation
  helpers that take `sys` explicitly.
- `crates/omni_utils/src/atomic_file.rs`: open, rename and remove through raw
  `system_traits` bounds.
- `crates/omni_remote_source/src/manager/default_impl.rs`: the lockfile's parent directory
  canonicalized through `FsCanonicalizeAsync` (#202).
