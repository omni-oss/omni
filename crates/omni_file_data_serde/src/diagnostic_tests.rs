use eyre::WrapErr;
use omni_diagnostic::{Audience, find};
use serde::Deserialize;

use crate::{Error, Format, ext_to_format, from_slice};

#[derive(Deserialize, Debug)]
struct Data {
    #[allow(dead_code)]
    test: u8,
}

fn wrapped(error: Error) -> eyre::Report {
    Err::<(), _>(error).wrap_err("loading data").unwrap_err()
}

#[test]
fn unsupported_extension_names_the_extension() {
    let r = wrapped(ext_to_format(std::ffi::OsStr::new("txt")).unwrap_err());
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "file_data.unsupported_extension");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["extension"], "txt");
    assert_eq!(d.hints().len(), 1);
}

#[test]
fn json_type_error_carries_the_path_and_location() {
    let err = from_slice::<Data>(br#"{"test": "hunter2"}"#, Format::Json)
        .unwrap_err();
    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "file_data.parse_failed");
    assert_eq!(d.audience(), Audience::User);
    let details = d.details().unwrap();
    assert_eq!(details["format"], "json");
    assert_eq!(details["path"], "test");
    assert_eq!(details["line"], 1);
    assert_eq!(details["column"], 18);
    assert!(!details.to_string().contains("hunter2"));
    assert!(!d.to_string().contains("hunter2"));
    assert!(!d.hints().join(" ").contains("hunter2"));
}

#[test]
fn yaml_syntax_error_carries_the_location_without_the_text() {
    let err = from_slice::<Data>(b"test: [\nsecret_token: \"x", Format::Yaml)
        .unwrap_err();
    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "file_data.parse_failed");
    let details = d.details().unwrap();
    assert_eq!(details["format"], "yaml");
    assert!(details["line"].as_u64().is_some_and(|l| l >= 1));
    assert!(!details.to_string().contains("secret_token"));
    assert!(!d.to_string().contains("secret_token"));
}

#[test]
fn toml_syntax_error_summary_is_generic() {
    let err = from_slice::<Data>(b"test = [", Format::Toml).unwrap_err();
    let r = wrapped(err);
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "file_data.parse_failed");
    assert_eq!(d.details().unwrap()["format"], "toml");
    assert_eq!(d.details().unwrap()["line"], 1);
    assert!(format!("{r:#}").contains("failed to parse TOML file (line 1"));
}

#[test]
fn plain_failures_stay_uncoded() {
    let utf8 = from_slice::<Data>(&[0xff, 0xfe], Format::Toml).unwrap_err();
    assert!(find(&wrapped(utf8)).is_none());

    let io = Error::from(std::io::Error::other("disk"));
    assert!(find(&wrapped(io)).is_none());
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in ["file_data.unsupported_extension", "file_data.parse_failed"] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
}

#[test]
fn registry_accepts_the_declared_prefix() {
    if let Err(errors) = omni_codes::validate() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("invalid code registry:\n{}", msgs.join("\n"));
    }
}
