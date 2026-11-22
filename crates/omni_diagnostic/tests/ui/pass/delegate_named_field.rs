use omni_diagnostic::Diagnostic;

#[allow(non_upper_case_globals)]
const __delegate: () = ();

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error("cannot read {path}")]
    #[diagnostic(delegate = r#type)]
    Named {
        path: String,
        r#type: std::io::Error,
    },

    #[error("cannot write {path}")]
    #[diagnostic(delegate = __out)]
    WriterCollision {
        path: String,
        #[source]
        __out: std::io::Error,
    },

    #[error("cannot read")]
    #[diagnostic(delegate)]
    Bare { source: std::io::Error },

    #[error("cannot open")]
    #[diagnostic(delegate)]
    BareTuple(std::io::Error),
}

fn main() {
    let () = __delegate;
}
