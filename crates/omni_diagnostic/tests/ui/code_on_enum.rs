use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(code = "x.a")]
enum E {
    #[error("a")]
    #[diagnostic(code = "x.b")]
    A,
}

fn main() {}
