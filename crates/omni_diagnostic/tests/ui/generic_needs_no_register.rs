use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("a {0}")]
#[diagnostic(code = "x.a")]
struct E<T: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static>(T);

fn main() {}
