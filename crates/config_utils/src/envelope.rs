//! Strict operator-envelope check shared by the list and dictionary decoders.

use serde_json::Value;

/// An operator envelope holds exactly its operator key. Any sibling would be
/// dropped by the decoder, so reject it instead of accepting a partial
/// declaration. An operator-named entry belongs inside an envelope
/// (`{merge: {replace: ...}}`), not beside other keys.
pub(crate) fn ensure_sole_operator(
    value: &Value,
    operator: &str,
) -> Result<(), String> {
    let Some(obj) = value.as_object() else {
        return Ok(());
    };

    let siblings = obj
        .keys()
        .filter(|k| k.as_str() != operator)
        .map(|k| format!("`{k}`"))
        .collect::<Vec<_>>();

    if siblings.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "`{operator}` must be the only key of an operator object, found {} beside it",
            siblings.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{DictConfig, ListConfig, Replace};

    type Dict = DictConfig<Replace<String>>;
    type List = ListConfig<Replace<String>>;

    #[test]
    fn dict_envelope_with_sibling_is_rejected() {
        let err = serde_json::from_str::<Dict>(
            r#"{"merge": {"a": "x"}, "hidden": "y"}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("`merge`") && err.contains("`hidden`"), "{err}");
    }

    #[test]
    fn dict_two_operators_are_rejected() {
        assert!(
            serde_json::from_str::<Dict>(
                r#"{"merge": {"a": "x"}, "replace": {"b": "y"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn list_envelope_with_extra_operator_is_rejected() {
        let err =
            serde_json::from_str::<List>(r#"{"append": [], "replace": ["x"]}"#)
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("`append`") && err.contains("`replace`"),
            "{err}"
        );
    }

    #[test]
    fn operator_named_entry_inside_an_envelope_is_an_entry() {
        let dict: Dict =
            serde_json::from_str(r#"{"merge": {"replace": "x"}}"#).unwrap();
        assert_eq!(
            dict,
            DictConfig::merge(maps::map! {
                "replace".to_string() => Replace::new("x".to_string())
            })
        );
    }

    #[test]
    fn valid_forms_decode_as_before() {
        let dict: Dict = serde_json::from_str(r#"{"a": "x"}"#).unwrap();
        assert!(dict.is_value());
        let dict: Dict = serde_json::from_str(r#"{"replace": {}}"#).unwrap();
        assert!(dict.is_replace());
        let list: List = serde_json::from_str(r#"{"prepend": ["x"]}"#).unwrap();
        assert!(list.is_prepend());
        let list: List = serde_json::from_str(r#"["x"]"#).unwrap();
        assert!(list.is_value());
    }
}
