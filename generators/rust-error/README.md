# Rust error diagnostics

Both `crate` and `rust-error` enable `use_diagnostics` by default. The crate
generator adds the `omni_diagnostic` workspace dependency and project dependency;
when invoking `rust-error` on an existing crate, add that Cargo dependency yourself.
Set `use_diagnostics` to `false` to generate errors without diagnostic integration.

Generated wrappers register a lookup that forwards diagnostics inside `Custom`
reports, including reports with `eyre::WrapErr` context. Plain custom messages
remain ordinary errors: they do not get a catch-all diagnostic code.

For a new user-facing failure, prefer a concrete diagnostic type with a meaningful
code, audience, and actionable hint:

```rust
#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error("unknown project `{name}`")]
#[diagnostic(code = "project.unknown", audience = user,
             hint = "Check the project name in your workspace configuration.")]
pub struct UnknownProject {
    #[diagnostic(detail)]
    pub name: String,
}
```

Add a variant to the generated inner enum and a corresponding wrapper lookup arm:

```rust,ignore
// ErrorInner:
#[error(transparent)]
UnknownProject(#[from] UnknownProject),

// register_wrapper! lookup:
ErrorInner::UnknownProject(inner) => Some(inner),
```

For enums whose variants all implement `Diagnostic`, derive `Diagnostic` on the
enum and use `#[diagnostic(forward)]` on each single-field forwarding variant.
This is distinct from `#[error(transparent)]`, which only forwards error behavior.
Do not derive `Diagnostic` on the generated `Custom(eyre::Report)` variant:
`eyre::Report` does not implement `Diagnostic`.

Explicit-registry builds must include a `wrapper_registration!` entry in their
installed registry; `register_wrapper!` only auto-registers with the inventory backend.
