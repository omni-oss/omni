use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("cannot read")]
    #[diagnostic(delegate = source, code = "sample.unreadable")]
    Code { source: std::io::Error },

    #[error("cannot write")]
    #[diagnostic(delegate = source, forward)]
    Forward { source: std::io::Error },

    #[error("cannot remove")]
    #[diagnostic(skip, delegate = source)]
    Skip { source: std::io::Error },

    #[error("cannot rename")]
    #[diagnostic(delegate = source, stable, related = "sample.other")]
    Metadata { source: std::io::Error },

    #[error("cannot copy")]
    #[diagnostic(delegate = source)]
    Detail {
        source: std::io::Error,
        #[diagnostic(detail)]
        path: String,
    },
}

#[derive(Debug, thiserror::Error, Diagnostic)]
enum Duplicate {
    #[error("cannot read")]
    #[diagnostic(delegate, delegate = source)]
    BareAndNamed { source: std::io::Error },
}

fn main() {}
