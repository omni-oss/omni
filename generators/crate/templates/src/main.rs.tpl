fn main() {
    let result = {{ inputs.crate_name }}::add(2, 2);
    println!("2 + 2 = {result}");
}
