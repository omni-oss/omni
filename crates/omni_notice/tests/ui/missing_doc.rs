use omni_notice::Notice;

#[derive(Notice)]
#[notice(code = "a.b", level = warn, message = "m")]
struct N;

fn main() {}
