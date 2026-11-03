use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a")]
#[diagnostic(code = "NotDotted")]
struct E;

fn main() {}
