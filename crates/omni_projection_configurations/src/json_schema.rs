use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::projection::{
    ExplicitRule, FlattenRule, PatternRule, Projection, ProjectionCommon,
};

impl JsonSchema for Projection {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Projection".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let common = common_properties(generator);

        let arms = vec![
            arm("namespaced", &common, Map::new(), &[]),
            arm("mirror", &common, scope_property(), &[]),
            arm(
                "explicit",
                &common,
                rules_property(schema_val::<ExplicitRule>(generator)),
                &["rules"],
            ),
            arm(
                "pattern",
                &common,
                rules_property(schema_val::<PatternRule>(generator)),
                &["rules"],
            ),
            arm(
                "flatten",
                &common,
                rules_property(schema_val::<FlattenRule>(generator)),
                &[],
            ),
        ];

        value_to_schema(json!({ "oneOf": arms }))
    }
}

/// Builds one strategy arm as a single flat object schema: the `strategy`
/// const, the shared `ProjectionCommon` fields, and any strategy-specific
/// fields, closed with `additionalProperties: false`.
///
/// The shared fields are inlined (rather than referenced through the
/// `ProjectionCommon` definition) so the arm carries exactly one
/// `additionalProperties: false`. Composing separate `additionalProperties:
/// false` object schemas with `allOf` is invalid: each closed schema would
/// reject the sibling arms' keys, so no instance could ever satisfy the arm.
fn arm(
    strategy: &str,
    common: &Map<String, Value>,
    extra: Map<String, Value>,
    extra_required: &[&str],
) -> Value {
    let mut properties = common.clone();
    properties.insert("strategy".into(), json!({ "const": strategy }));
    properties.extend(extra);

    let mut required = vec![json!("strategy")];
    required.extend(extra_required.iter().map(|r| json!(r)));

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// The `properties` of [`ProjectionCommon`], inlined so the shared fields can be
/// merged into each arm without dragging along the definition's own
/// `additionalProperties: false`.
fn common_properties(generator: &mut SchemaGenerator) -> Map<String, Value> {
    let schema = serde_json::to_value(ProjectionCommon::json_schema(generator))
        .expect("ProjectionCommon schema serializes");
    schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn scope_property() -> Map<String, Value> {
    object(json!({ "scope": { "type": "string" } }))
}

fn rules_property(item_schema: Value) -> Map<String, Value> {
    object(json!({
        "rules": { "type": "array", "items": item_schema }
    }))
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => panic!("expected a JSON object"),
    }
}

fn schema_val<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    serde_json::to_value(generator.subschema_for::<T>())
        .expect("Schema is always valid JSON")
}

fn value_to_schema(v: Value) -> Schema {
    match v {
        Value::Object(map) => Schema::from(map),
        Value::Bool(b) => Schema::from(b),
        _ => panic!("expected JSON object or bool for Schema"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection_schema() -> Value {
        let generator = SchemaGenerator::default();
        serde_json::to_value(generator.into_root_schema_for::<Projection>())
            .expect("schema serializes")
    }

    fn one_of(schema: &Value) -> &Vec<Value> {
        schema
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("Projection schema is a oneOf")
    }

    fn strategy_const(arm: &Value) -> &str {
        arm.pointer("/properties/strategy/const")
            .and_then(Value::as_str)
            .expect("arm pins a strategy const")
    }

    #[test]
    fn schema_is_one_of_five_strategy_arms() {
        let schema = projection_schema();
        let arms = one_of(&schema);
        assert_eq!(arms.len(), 5, "one arm per strategy");

        let mut strategies: Vec<&str> =
            arms.iter().map(strategy_const).collect();
        strategies.sort_unstable();
        assert_eq!(
            strategies,
            ["explicit", "flatten", "mirror", "namespaced", "pattern"]
        );
    }

    #[test]
    fn every_arm_carries_the_shared_common_fields() {
        let schema = projection_schema();
        for arm in one_of(&schema) {
            let props = arm
                .pointer("/properties")
                .and_then(Value::as_object)
                .expect("arm is an object schema");
            for field in [
                "target",
                "on_collision",
                "link",
                "allow_omni_config",
                "allow_git",
            ] {
                assert!(
                    props.contains_key(field),
                    "{} arm should carry `{field}`",
                    strategy_const(arm)
                );
            }
        }
    }

    #[test]
    fn every_arm_is_closed_to_unknown_fields() {
        let schema = projection_schema();
        for arm in one_of(&schema) {
            assert_eq!(
                arm.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "{} arm must reject unknown fields",
                strategy_const(arm)
            );
        }
    }

    #[test]
    fn only_pattern_and_flatten_rule_defs_carry_match_kind() {
        let schema = projection_schema();
        let defs = schema
            .get("$defs")
            .and_then(Value::as_object)
            .expect("root schema has $defs");

        let mentions_match_kind = |value: &Value| {
            let text = serde_json::to_string(value).unwrap();
            text.contains("match_kind") || text.contains("match-kind")
        };

        assert!(
            mentions_match_kind(&defs["PatternRule"]),
            "PatternRule should expose match-kind"
        );
        assert!(
            mentions_match_kind(&defs["FlattenRule"]),
            "FlattenRule should expose match-kind"
        );
        assert!(
            !mentions_match_kind(&defs["ExplicitRule"]),
            "ExplicitRule must not expose match-kind"
        );
    }

    #[test]
    fn rule_bearing_arms_reference_their_rule_type() {
        let schema = projection_schema();
        for arm in one_of(&schema) {
            let strategy = strategy_const(arm);
            let arm_text = serde_json::to_string(arm).unwrap();
            let expected_ref = match strategy {
                "explicit" => Some("ExplicitRule"),
                "pattern" => Some("PatternRule"),
                "flatten" => Some("FlattenRule"),
                _ => None,
            };
            if let Some(name) = expected_ref {
                assert!(
                    arm_text.contains(name),
                    "{strategy} arm should reference {name}"
                );
            }
        }
    }
}
