use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("a")]
    #[diagnostic(skip, delegate)]
    A(std::io::Error),
}

fn main() {}
