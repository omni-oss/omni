use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("a")]
    #[diagnostic(code = "x.same")]
    A,
    #[error("b")]
    #[diagnostic(code = "x.same")]
    B,
}

fn main() {}
