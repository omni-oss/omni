# Errors

Rust crates define typed errors with `thiserror`, deriving `omni_diagnostic::Diagnostic` for user-facing diagnostics. Enum names and discriminants come from `strum`. Generate new errors with the `rust-error` generator. On an existing crate, add the `omni_diagnostic` dependency yourself first (see `generators/rust-error/README.md`).

Examples with the diagnostic derive: `crates/env_loader/src/error.rs`, `crates/omni_capabilities/src/error.rs`. Plain `thiserror`: `crates/omni_git_utils/src/error.rs`, `crates/bridge_rpc_core/src/bridge/client/request_error.rs`.

## Preserve diagnostic codes across wrappers

Diagnostic lookup (`omni_diagnostic::find`) walks the error's cause chain and recognizes only types that registered a lookup, which the `Diagnostic` derive does. `#[error(transparent)]` makes the wrapper stand in for the inner error: it forwards `Display` and also `source()`, but `source()` returns the inner error's _source_, not the inner error. The inner error is a field of the wrapper, not a link in the chain, so it is never visited.

Rust has no reflection, so `find` cannot look inside a type that did not register itself. Changing `find` to check sources more thoroughly would not help.

### The failure

```rust
#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error("invalid shell syntax")]
#[diagnostic(code = "command.invalid_shell_syntax", hint = "check quoting")]
pub struct InvalidShellSyntax;

// Compiles and prints the right message, but hides the code.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ExecutionPipelineError(#[from] InvalidShellSyntax);

let report: eyre::Report = ExecutionPipelineError(InvalidShellSyntax).into();
assert!(omni_diagnostic::find(&report).is_none()); // the code is gone
```

At the CLI this prints a plain `error: invalid shell syntax` and exits `1`, instead of `error[command.invalid_shell_syntax]` with the hint and exit `2`. Nothing fails at build time.

### The fixes

- For a transparent wrapper over a diagnosable error, derive `Diagnostic` and use `#[diagnostic(forward)]` on a single-field struct or `#[diagnostic(delegate)]` on a single-field enum variant:

    ```rust
    #[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
    #[error(transparent)]
    #[diagnostic(forward)]
    pub struct ExecutionPipelineError(#[from] InvalidShellSyntax);
    ```

    See [`ProjectConfigLoaderError` and its `LoadConfig` variant](../../crates/omni_context/src/project_config_loader.rs).

- To add context without defining a new diagnostic, give the wrapper its own `#[error("...")]` message and keep the inner error in a named `#[source]` field. That puts the cause in the chain instead of skipping over it.

### What is and is not checked

The derive requires an explicit choice (`code`, `forward`, `delegate` or `skip`) on every type that uses it. It cannot check a wrapper that omits the derive, because the macro never runs on it. Nothing in CI scans for such wrappers.

When you add or change a wrapper on a user-facing error path, test the stable code and exit status at the CLI boundary rather than checking the wrapper's attributes. See `command.invalid_shell_syntax` in `tests/omni-tests/src/command-config.e2e.spec.ts`.
