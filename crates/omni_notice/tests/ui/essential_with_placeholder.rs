use omni_notice::Notice;

/// Doc.
#[derive(Notice)]
#[notice(code = "a.b", level = warn, essential, message = "m {x}")]
struct N;

fn main() {}
