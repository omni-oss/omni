use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("cannot read {path}")]
    #[diagnostic(delegate = missing)]
    Named {
        path: String,
        source: std::io::Error,
    },
}

fn main() {}
