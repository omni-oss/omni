use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a")]
#[diagnostic(code = "x.a", audience = everyone)]
struct E;

fn main() {}
