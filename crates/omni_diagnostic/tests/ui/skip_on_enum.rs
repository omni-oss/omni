use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(skip)]
enum E {
    #[error("a")]
    #[diagnostic(code = "x.a")]
    A,
}

fn main() {}
