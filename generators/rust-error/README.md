# Rust error diagnostics

Both `crate` and `rust-error` enable `use_diagnostics` by default. The crate
generator adds the `omni_diagnostic` workspace dependency and project dependency;
when invoking `rust-error` on an existing crate, add that Cargo dependency yourself.
Set `use_diagnostics` to `false` to generate errors without diagnostic integration.

The generated public error forwards to its inner enum, which derives
`omni_diagnostic::Diagnostic`:

```rust
#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

#[derive(Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic)]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),
}
```

`Custom` uses `delegate`, so a diagnostic inside the report (including one
wrapped with `eyre::WrapErr` context) is still found. A plain custom message stays an
ordinary error and gets no catch-all code.

The derive requires every variant to say what it is, so a new variant will not compile
until you decide:

| Attribute                                | Use for                                                                                                                                                            |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `#[diagnostic(code = "area.name", ...)]` | a failure the user or an agent can act on; gets a stable code, audience, details and hints                                                                         |
| `#[diagnostic(delegate)]`                | a variant wrapping another error, or an `eyre::Report`, that may carry its own diagnostic; it is looked up in the field (a report's whole cause chain is searched) |
| `#[diagnostic(skip)]`                    | a failure with no diagnostic (I/O, serialization, internal plumbing)                                                                                               |
| `#[diagnostic(forward)]`                 | a variant wrapping a type that itself implements `Diagnostic`                                                                                                      |

Example:

```rust
pub(crate) enum ErrorInner {
    /// The project could not be found.
    #[error("unknown project `{name}`")]
    #[diagnostic(code = "project.unknown", audience = user,
                 hint = "Check the project name in your workspace configuration.")]
    UnknownProject {
        #[diagnostic(detail)]
        name: String,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),
}
```

The first doc line of a coded variant becomes its summary in the code catalog. Codes
must be unique across the workspace; `cargo test -p omni_cli_core --test code_registry`
checks that and that every registered code is well formed.

Do not hand-write `impl Diagnostic`. A manual impl is not registered in the code
catalog and skips the derive's checks.
