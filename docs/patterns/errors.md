# Errors

Rust crates define typed errors with `thiserror`, deriving `omni_diagnostic::Diagnostic` for user-facing diagnostics. Enum names and discriminants come from `strum`. Generate new errors with the `rust-error` generator.

Examples: `crates/omni_git_utils/src/error.rs`, `crates/bridge_rpc_core/src/bridge/client/request_error.rs`.
