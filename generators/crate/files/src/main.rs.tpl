fn main() {
    let result = {{ prompts.crate_name }}::add(2, 2);
    println!("2 + 2 = {result}");
}
