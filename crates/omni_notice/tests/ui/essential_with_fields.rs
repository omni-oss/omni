use omni_notice::Notice;

/// Doc.
#[derive(Notice)]
#[notice(code = "a.b", level = warn, essential, message = "m")]
struct N { x: u8 }

fn main() {}
