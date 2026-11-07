use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("a")]
    #[diagnostic(delegate)]
    A(std::io::Error, String),
}

fn main() {}
