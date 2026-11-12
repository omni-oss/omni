//! Parity: serde and JSON-schema output of source configurations must stay
//! byte-identical across the extraction into `omni_source_configurations`.
use omni_configurations::{
    GeneratorSourceConfiguration, PackSourceConfiguration,
    ProjectionSourceConfiguration,
};
use schemars::schema_for;

fn check(name: &str, actual: String) {
    let path =
        format!("{}/tests/snapshots/{name}.json", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(expected, actual, "snapshot {name} drifted");
}

#[test]
fn null_profile_schema_is_unchanged() {
    let s = schema_for!(GeneratorSourceConfiguration);
    check(
        "source_null_schema",
        serde_json::to_string_pretty(&s).unwrap(),
    );
}

#[test]
fn projection_profile_schema_is_unchanged() {
    let s = schema_for!(ProjectionSourceConfiguration);
    check(
        "source_projection_schema",
        serde_json::to_string_pretty(&s).unwrap(),
    );
}

#[test]
fn pack_profile_schema_is_unchanged() {
    let s = schema_for!(PackSourceConfiguration);
    check(
        "source_pack_schema",
        serde_json::to_string_pretty(&s).unwrap(),
    );
}

#[test]
fn serde_output_is_unchanged() {
    let json = r#"{"source":"local","path":["./a","./b"]}"#;
    let v: GeneratorSourceConfiguration = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
    let json =
        r#"{"source":"git","uri":"https://example.com/a.git","rev":"v1"}"#;
    let v: GeneratorSourceConfiguration = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&v).unwrap(), json);
}
