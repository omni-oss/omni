use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("cannot read")]
    #[diagnostic(delegate = _0)]
    Tuple(std::io::Error),

    #[error("cannot write")]
    #[diagnostic(delegate = source)]
    Unit,
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("cannot read")]
#[diagnostic(delegate = source)]
struct Named {
    source: std::io::Error,
}

fn main() {}
