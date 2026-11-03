use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a")]
#[diagnostic(code = "x.a")]
struct E(#[diagnostic(detail)] String);

fn main() {}
