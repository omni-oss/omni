# Errors

Rust crates define typed errors with `thiserror`, deriving `omni_diagnostic::Diagnostic` for user-facing diagnostics. Enum names and discriminants come from `strum`. Generate new errors with the `rust-error` generator. On an existing crate, add the `omni_diagnostic` dependency yourself first (see `generators/rust-error/README.md`).

Examples with the diagnostic derive: `crates/env_loader/src/error.rs`, `crates/omni_capabilities/src/error.rs`. Plain `thiserror`: `crates/omni_git_utils/src/error.rs`, `crates/bridge_rpc_core/src/bridge/client/request_error.rs`.
