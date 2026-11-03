use omni_diagnostic::Diagnostic;

#[derive(Debug, thiserror::Error)]
#[error("i")]
struct I;
impl Diagnostic for I { fn code(&self) -> &'static str { "x.i" } }

#[derive(Debug, thiserror::Error, Diagnostic)]
enum E {
    #[error(transparent)]
    #[diagnostic(forward, code = "x.a")]
    A(I),
}

fn main() {}
