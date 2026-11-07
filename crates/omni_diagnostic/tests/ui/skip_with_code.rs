use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("a")]
    #[diagnostic(skip, code = "x.a")]
    A,
}

fn main() {}
