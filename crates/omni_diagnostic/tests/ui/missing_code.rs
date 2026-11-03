use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("a")]
    #[diagnostic(code = "x.a")]
    A,
    #[error("b")]
    B,
}

fn main() {}
