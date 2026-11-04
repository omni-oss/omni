#[test]
fn compile_rules() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
