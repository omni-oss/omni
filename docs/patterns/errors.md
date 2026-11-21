# Errors

Rust crates define typed errors with `thiserror`, deriving `omni_diagnostic::Diagnostic` for user-facing diagnostics. Enum names and discriminants come from `strum`. Generate new errors with the `rust-error` generator. On an existing crate, add the `omni_diagnostic` dependency yourself first (see `generators/rust-error/README.md`).

Examples with the diagnostic derive: `crates/env_loader/src/error.rs`, `crates/omni_capabilities/src/error.rs`. Plain `thiserror`: `crates/omni_git_utils/src/error.rs`, `crates/bridge_rpc_core/src/bridge/client/request_error.rs`.

## Preserve diagnostic codes across wrappers

Diagnostic lookup walks the error's cause chain. `#[error(transparent)]` forwards `source()` to the inner error's source, so it can hide the inner error that carries the code.

- For a transparent wrapper over a diagnosable error, derive `Diagnostic` and use `#[diagnostic(forward)]` on a single-field struct or `#[diagnostic(delegate)]` on a single-field enum variant. See [`ProjectConfigLoaderError` and its `LoadConfig` variant](../../crates/omni_context/src/project_config_loader.rs).
- To add context without defining a new diagnostic, give the wrapper its own `#[error("...")]` message and keep the inner error in a named `#[source]` field. That exposes the cause to diagnostic lookup instead of skipping over it.

The derive requires an explicit choice on types that use it; it cannot check wrappers that omit it. When changing a wrapper on a user-facing error path, test the stable code and exit status at the CLI boundary rather than checking the wrapper's attributes.
