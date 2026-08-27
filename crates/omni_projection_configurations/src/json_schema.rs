use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde_json::{Value, json};

use crate::projection::{
    ExplicitRule, FlattenRule, PatternRule, Projection, ProjectionCommon,
};

impl JsonSchema for Projection {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Projection".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let common = schema_val::<ProjectionCommon>(generator);

        let arms = vec![
            arm("namespaced", &common, Vec::new()),
            arm(
                "mirror",
                &common,
                vec![props(json!({
                    "scope": { "type": "string" }
                }))],
            ),
            arm(
                "explicit",
                &common,
                vec![rules_prop(
                    schema_val::<ExplicitRule>(generator),
                    true,
                )],
            ),
            arm(
                "pattern",
                &common,
                vec![rules_prop(
                    schema_val::<PatternRule>(generator),
                    true,
                )],
            ),
            arm(
                "flatten",
                &common,
                vec![rules_prop(
                    schema_val::<FlattenRule>(generator),
                    false,
                )],
            ),
        ];

        value_to_schema(json!({ "oneOf": arms }))
    }
}

/// Builds the `allOf` schema for one strategy arm: the `strategy` const, the
/// shared `ProjectionCommon` schema, then any strategy-specific fields.
fn arm(strategy: &str, common: &Value, extras: Vec<Value>) -> Value {
    let mut all_of = vec![
        json!({
            "type": "object",
            "properties": { "strategy": { "const": strategy } },
            "required": ["strategy"]
        }),
        common.clone(),
    ];
    all_of.extend(extras);
    json!({ "allOf": all_of })
}

/// A `rules` array whose items are `item_schema`, marked required when the
/// strategy demands at least the key be present.
fn rules_prop(item_schema: Value, required: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "rules": { "type": "array", "items": item_schema }
        }
    });
    if required {
        schema["required"] = json!(["rules"]);
    }
    schema
}

fn props(properties: Value) -> Value {
    json!({ "type": "object", "properties": properties })
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
        arm.get("allOf")
            .and_then(Value::as_array)
            .expect("arm is an allOf")
            .iter()
            .find_map(|clause| {
                clause
                    .pointer("/properties/strategy/const")
                    .and_then(Value::as_str)
            })
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

    fn mentions_match_kind(value: &Value) -> bool {
        let text = serde_json::to_string(value).unwrap();
        text.contains("match_kind") || text.contains("match-kind")
    }

    #[test]
    fn only_pattern_and_flatten_rule_defs_carry_match_kind() {
        let schema = projection_schema();
        let defs = schema
            .get("$defs")
            .and_then(Value::as_object)
            .expect("root schema has $defs");

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
