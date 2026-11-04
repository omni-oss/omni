use omni_notice::Notice;

/// Doc.
#[derive(Debug, Notice)]
#[notice(code = "a.b", level = warn, message = "m")]
struct N;

impl std::error::Error for N {}

fn main() {}
