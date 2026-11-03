use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a")]
#[diagnostic(code = "x.a", hint = "try {nope}")]
struct E {
    name: String,
}

fn main() {}
