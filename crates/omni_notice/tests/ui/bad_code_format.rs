use omni_notice::Notice;

/// Doc.
#[derive(Notice)]
#[notice(code = "NoDots", level = warn, message = "m")]
struct N;

fn main() {}
