use omni_diagnostic::Diagnostic;

fn d(_: &E) -> serde_json::Value { serde_json::Value::Null }

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a")]
#[diagnostic(code = "x.a", details = d)]
struct E {
    #[diagnostic(detail)]
    name: String,
}

fn main() {}
