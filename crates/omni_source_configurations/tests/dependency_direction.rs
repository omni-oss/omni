//! This crate is a leaf below `omni_configurations`; depending on it would
//! create a cycle for future plugin configuration crates.
#[test]
fn does_not_depend_on_omni_configurations() {
    let manifest = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .unwrap();
    let forbidden = ["omni_configurations", "omni_plugin"];
    for line in manifest.lines() {
        let name = line.split('=').next().unwrap().trim();
        assert!(
            !forbidden.iter().any(|f| name.starts_with(f)),
            "forbidden dependency: {line}"
        );
    }
}
