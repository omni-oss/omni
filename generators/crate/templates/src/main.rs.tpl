// A binary's own output is its result, so `println!` is allowed here. Library
// crates report through return values, events, errors, notices or `tracing`.
#![allow(clippy::disallowed_macros)]

fn main() {
    let result = {{ inputs.crate_name }}::add(2, 2);
    println!("2 + 2 = {result}");
}
