#![cfg(feature = "schemars")]

use omni_path_utils::{ProjectRootId, WorkspacePath};
use schemars::schema_for;

#[test]
fn serde_round_trip() {
    let p = WorkspacePath::parse("apps/Web").unwrap();
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, "\"apps/Web\"");
    assert_eq!(serde_json::from_str::<WorkspacePath>(&json).unwrap(), p);

    let id = ProjectRootId::new(p);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"apps/Web\"");
    assert_eq!(serde_json::from_str::<ProjectRootId>(&json).unwrap(), id);

    let root: WorkspacePath = serde_json::from_str("\".\"").unwrap();
    assert!(root.is_root());
}

#[test]
fn wire_input_is_rejected_not_repaired() {
    for bad in [
        "\"\"",
        "\"./a\"",
        "\"a/\"",
        "\"a//b\"",
        "\"..\"",
        "\"a\\\\b\"",
        "\"/a\"",
        "\"C:\\\\a\"",
        "\"a\\u0000b\"",
    ] {
        assert!(serde_json::from_str::<WorkspacePath>(bad).is_err(), "{bad}");
        assert!(serde_json::from_str::<ProjectRootId>(bad).is_err(), "{bad}");
    }
    assert!(serde_json::from_str::<WorkspacePath>("1").is_err());
}

#[test]
fn as_map_key_and_in_structs() {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct S {
        root: ProjectRootId,
        inputs: Vec<WorkspacePath>,
    }
    let s = S {
        root: WorkspacePath::parse("a").unwrap().into(),
        inputs: vec![WorkspacePath::root()],
    };
    let j = serde_json::to_value(&s).unwrap();
    assert_eq!(j, serde_json::json!({"root": "a", "inputs": ["."]}));
    assert_eq!(serde_json::from_value::<S>(j).unwrap(), s);
}

#[test]
fn schema_is_bounded_string() {
    for schema in [schema_for!(WorkspacePath), schema_for!(ProjectRootId)] {
        let v = serde_json::to_value(&schema).unwrap();
        assert_eq!(v["type"], "string");
        assert_eq!(v["minLength"], 1);
        assert_eq!(v["maxLength"], 4096);
    }
}
