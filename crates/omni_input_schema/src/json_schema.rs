use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde_json::{Value, json};

use crate::allowed::{AllowedValue, ArrayBody};
use crate::base::BaseInput;
use crate::input::{Input, InputKind};
use crate::profile::InputProfile;

// ── Manual JsonSchema impl ────────────────────────────────────────────────────

impl<E: InputProfile> JsonSchema for Input<E>
where
    E::Base: JsonSchema,
    E::Boolean: JsonSchema,
    E::String: JsonSchema,
    E::Numeric: JsonSchema,
    E::Array: JsonSchema,
    E::Object: JsonSchema,
    E::AllowedValueBase: JsonSchema,
    AllowedValue<std::string::String, E>: JsonSchema,
    AllowedValue<i64, E>: JsonSchema,
    AllowedValue<f64, E>: JsonSchema,
{
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Input".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        // iterate only E::SUPPORTED so unsupported variants
        // (e.g. Object for Generator) are absent from the schema by construction.
        let arms: Vec<Value> = E::SUPPORTED
            .iter()
            .map(|kind| match kind {
                InputKind::Boolean => boolean_arm::<E>(generator),
                InputKind::String => string_arm::<E>(generator),
                InputKind::Integer => integer_arm::<E>(generator),
                InputKind::Float => float_arm::<E>(generator),
                InputKind::StringArray => string_array_arm::<E>(generator),
                InputKind::IntegerArray => integer_array_arm::<E>(generator),
                InputKind::FloatArray => float_array_arm::<E>(generator),
                InputKind::Object => object_arm::<E>(generator),
            })
            .collect();

        value_to_schema(json!({ "oneOf": arms }))
    }
}

// ── Schema helpers ───────────────────────────────────────────────────────────

fn value_to_schema(v: serde_json::Value) -> Schema {
    match v {
        serde_json::Value::Object(map) => Schema::from(map),
        serde_json::Value::Bool(b) => Schema::from(b),
        _ => panic!("expected JSON object or bool for Schema"),
    }
}

// ── Arm-schema helpers ────────────────────────────────────────────────────────

/// Builds the allOf schema for one variant arm.  The serde tag is emitted as a
/// const-string property so the discriminant value matches the serde output
/// exactly.
fn make_arm(tag_value: &str, base: Value, extras: Vec<Value>) -> Value {
    let mut all_of = vec![
        json!({
            "type": "object",
            "properties": { "type": { "const": tag_value } },
            "required": ["type"]
        }),
        base,
    ];
    all_of.extend(extras);
    json!({ "allOf": all_of })
}

fn schema_val<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    serde_json::to_value(generator.subschema_for::<T>())
        .expect("Schema is always valid JSON")
}

fn boolean_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Boolean: JsonSchema,
{
    let props = json!({
        "type": "object",
        "properties": { "default": { "type": "boolean" } }
    });
    make_arm(
        "boolean",
        schema_val::<BaseInput>(generator),
        vec![
            props,
            schema_val::<E::Base>(generator),
            schema_val::<E::Boolean>(generator),
        ],
    )
}

fn string_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::String: JsonSchema,
    AllowedValue<std::string::String, E>: JsonSchema,
{
    let allowed_schema =
        schema_val::<AllowedValue<std::string::String, E>>(generator);
    let props = json!({
        "type": "object",
        "properties": {
            "allowed": { "type": "array", "items": allowed_schema },
            "default": { "type": "string" }
        }
    });
    make_arm(
        "string",
        schema_val::<BaseInput>(generator),
        vec![
            props,
            schema_val::<E::Base>(generator),
            schema_val::<E::String>(generator),
        ],
    )
}

fn integer_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Numeric: JsonSchema,
    AllowedValue<i64, E>: JsonSchema,
{
    let allowed_schema = schema_val::<AllowedValue<i64, E>>(generator);
    let props = json!({
        "type": "object",
        "properties": {
            "allowed": { "type": "array", "items": allowed_schema },
            "default": { "type": "integer" }
        }
    });
    make_arm(
        "integer",
        schema_val::<BaseInput>(generator),
        vec![
            props,
            schema_val::<E::Base>(generator),
            schema_val::<E::Numeric>(generator),
        ],
    )
}

fn float_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Numeric: JsonSchema,
    AllowedValue<f64, E>: JsonSchema,
{
    let allowed_schema = schema_val::<AllowedValue<f64, E>>(generator);
    let props = json!({
        "type": "object",
        "properties": {
            "allowed": { "type": "array", "items": allowed_schema },
            "default": { "type": "number" }
        }
    });
    make_arm(
        "float",
        schema_val::<BaseInput>(generator),
        vec![
            props,
            schema_val::<E::Base>(generator),
            schema_val::<E::Numeric>(generator),
        ],
    )
}

fn string_array_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Array: JsonSchema,
    AllowedValue<std::string::String, E>: JsonSchema,
{
    let body_schema =
        schema_val::<ArrayBody<std::string::String, E>>(generator);
    make_arm(
        "string-array",
        schema_val::<BaseInput>(generator),
        vec![
            body_schema,
            schema_val::<E::Base>(generator),
            schema_val::<E::Array>(generator),
        ],
    )
}

fn integer_array_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Array: JsonSchema,
    AllowedValue<i64, E>: JsonSchema,
{
    let body_schema = schema_val::<ArrayBody<i64, E>>(generator);
    make_arm(
        "integer-array",
        schema_val::<BaseInput>(generator),
        vec![
            body_schema,
            schema_val::<E::Base>(generator),
            schema_val::<E::Array>(generator),
        ],
    )
}

fn float_array_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Array: JsonSchema,
    AllowedValue<f64, E>: JsonSchema,
{
    let body_schema = schema_val::<ArrayBody<f64, E>>(generator);
    make_arm(
        "float-array",
        schema_val::<BaseInput>(generator),
        vec![
            body_schema,
            schema_val::<E::Base>(generator),
            schema_val::<E::Array>(generator),
        ],
    )
}

fn object_arm<E: InputProfile>(generator: &mut SchemaGenerator) -> Value
where
    E::Base: JsonSchema,
    E::Object: JsonSchema,
{
    let fields_item_schema =
        serde_json::to_value(generator.subschema_for::<Input<E>>())
            .expect("Schema is always valid JSON");
    let props = json!({
        "type": "object",
        "properties": {
            "fields": { "type": "array", "items": fields_item_schema }
        },
        "required": ["fields"]
    });
    make_arm(
        "object",
        schema_val::<BaseInput>(generator),
        vec![
            props,
            schema_val::<E::Base>(generator),
            schema_val::<E::Object>(generator),
        ],
    )
}

// ── to_json_schema ────────────────────────────────────────────────────────────

/// Generate a JSON Schema for the resolved-values object of a set of inputs.
///
/// Produces `{ "type": "object", "properties": { … }, "required": [ … ] }` where
/// each property is derived from the data type of the corresponding `Input<E>`.
///
/// - `secret: true` maps to `writeOnly: true` (+ `format: "password"` on strings).
/// - `allowed` values produce an `"enum"` constraint; if any carries a
///   `description`, the projection uses `oneOf` of `{ const, description }` objects.
/// - The `"required"` list contains only active inputs (no always-hidden condition)
///   that have no static default.
pub fn to_json_schema<E: InputProfile>(inputs: &[Input<E>]) -> Value {
    let mut properties: serde_json::Map<std::string::String, Value> =
        Default::default();
    let mut required: Vec<Value> = Vec::new();

    for input in inputs {
        let base = input.base();
        let prop_schema = input_to_property_schema(input);
        properties.insert(base.name.clone(), prop_schema);

        let always_hidden =
            matches!(&base.r#if, Some(either::Either::Left(false)));
        let has_default = input.default_value_bag().is_some();
        if !always_hidden && !has_default {
            required.push(Value::String(base.name.clone()));
        }
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

fn input_to_property_schema<E: InputProfile>(input: &Input<E>) -> Value {
    let base = input.base();
    let mut schema: Value = match input {
        Input::Boolean(_) => json!({ "type": "boolean" }),
        Input::String(s) => {
            let mut schema = json!({ "type": "string" });
            if let Some(allowed) = &s.allowed {
                apply_string_allowed(
                    &mut schema,
                    allowed
                        .iter()
                        .map(|a| (&a.value, a.description.as_deref())),
                );
            }
            if base.secret {
                schema["format"] = json!("password");
            }
            schema
        }
        Input::Integer(i) => {
            let mut schema = json!({ "type": "integer" });
            if let Some(allowed) = &i.allowed {
                apply_numeric_allowed(
                    &mut schema,
                    allowed
                        .iter()
                        .map(|a| (json!(a.value), a.description.as_deref())),
                );
            }
            schema
        }
        Input::Float(f) => {
            let mut schema = json!({ "type": "number" });
            if let Some(allowed) = &f.allowed {
                apply_numeric_allowed(
                    &mut schema,
                    allowed
                        .iter()
                        .map(|a| (json!(a.value), a.description.as_deref())),
                );
            }
            schema
        }
        Input::StringArray(sa) => {
            let mut items = json!({ "type": "string" });
            if let Some(allowed) = &sa.body.allowed {
                apply_string_allowed(
                    &mut items,
                    allowed
                        .iter()
                        .map(|a| (&a.value, a.description.as_deref())),
                );
            }
            json!({ "type": "array", "items": items })
        }
        Input::IntegerArray(ia) => {
            let mut items = json!({ "type": "integer" });
            if let Some(allowed) = &ia.body.allowed {
                apply_numeric_allowed(
                    &mut items,
                    allowed
                        .iter()
                        .map(|a| (json!(a.value), a.description.as_deref())),
                );
            }
            json!({ "type": "array", "items": items })
        }
        Input::FloatArray(fa) => {
            let mut items = json!({ "type": "number" });
            if let Some(allowed) = &fa.body.allowed {
                apply_numeric_allowed(
                    &mut items,
                    allowed
                        .iter()
                        .map(|a| (json!(a.value), a.description.as_deref())),
                );
            }
            json!({ "type": "array", "items": items })
        }
        Input::Object(o) => {
            let nested = to_json_schema(&o.fields);
            let mut s = nested;
            if o.base.secret {
                s["writeOnly"] = json!(true);
            }
            if let Some(desc) = &o.base.description {
                s["description"] = json!(desc);
            }
            return s;
        }
    };

    if base.secret {
        schema["writeOnly"] = json!(true);
    }
    if let Some(desc) = &base.description {
        schema["description"] = json!(desc);
    }
    schema
}

/// Apply an `enum` or `oneOf` constraint for string allowed values.
///
/// When any entry carries a `description`, uses `oneOf` of `{ const, description }`
/// objects so per-option help reaches machine consumers.  Otherwise uses a plain
/// `enum` array.
fn apply_string_allowed<'a>(
    schema: &mut Value,
    entries: impl Iterator<Item = (&'a String, Option<&'a str>)>,
) {
    let entries: Vec<_> = entries.collect();
    let has_desc = entries.iter().any(|(_, d)| d.is_some());
    if has_desc {
        let one_of: Vec<Value> = entries
            .into_iter()
            .map(|(v, d)| {
                let mut obj = json!({ "const": v });
                if let Some(desc) = d {
                    obj["description"] = json!(desc);
                }
                obj
            })
            .collect();
        schema["oneOf"] = json!(one_of);
    } else {
        let values: Vec<Value> =
            entries.into_iter().map(|(v, _)| json!(v)).collect();
        schema["enum"] = json!(values);
    }
}

/// Same as `apply_string_allowed` but for numeric (already-`Value`) entries.
fn apply_numeric_allowed<'a>(
    schema: &mut Value,
    entries: impl Iterator<Item = (Value, Option<&'a str>)>,
) {
    let entries: Vec<_> = entries.collect();
    let has_desc = entries.iter().any(|(_, d)| d.is_some());
    if has_desc {
        let one_of: Vec<Value> = entries
            .into_iter()
            .map(|(v, d)| {
                let mut obj = json!({ "const": v });
                if let Some(desc) = d {
                    obj["description"] = json!(desc);
                }
                obj
            })
            .collect();
        schema["oneOf"] = json!(one_of);
    } else {
        let values: Vec<Value> = entries.into_iter().map(|(v, _)| v).collect();
        schema["enum"] = json!(values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Input, InputKind};
    use crate::profile::InputProfile;
    use enumset::EnumSet;

    fn parse<T: for<'de> serde::Deserialize<'de>>(json: &str) -> T {
        serde_json::from_str(json).expect(json)
    }

    // ── to_json_schema: scalar types ─────────────────────────────────────────

    #[test]
    fn boolean_emits_type_boolean() {
        let input: Input<()> = parse(r#"{"type":"boolean","name":"flag"}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["flag"]["type"], "boolean");
        assert_eq!(schema["required"][0], "flag");
    }

    #[test]
    fn string_emits_type_string() {
        let input: Input<()> = parse(r#"{"type":"string","name":"label"}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["label"]["type"], "string");
    }

    #[test]
    fn integer_emits_type_integer() {
        let input: Input<()> = parse(r#"{"type":"integer","name":"count"}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn float_emits_type_number() {
        let input: Input<()> = parse(r#"{"type":"float","name":"rate"}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["rate"]["type"], "number");
    }

    // ── to_json_schema: array types ──────────────────────────────────────────

    #[test]
    fn string_array_emits_array_with_string_items() {
        let input: Input<()> =
            parse(r#"{"type":"string-array","name":"tags"}"#);
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["tags"];
        assert_eq!(prop["type"], "array");
        assert_eq!(prop["items"]["type"], "string");
    }

    #[test]
    fn integer_array_emits_array_with_integer_items() {
        let input: Input<()> =
            parse(r#"{"type":"integer-array","name":"ids"}"#);
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["ids"];
        assert_eq!(prop["type"], "array");
        assert_eq!(prop["items"]["type"], "integer");
    }

    #[test]
    fn float_array_emits_array_with_number_items() {
        let input: Input<()> = parse(r#"{"type":"float-array","name":"vals"}"#);
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["vals"];
        assert_eq!(prop["type"], "array");
        assert_eq!(prop["items"]["type"], "number");
    }

    // ── to_json_schema: Object type ──────────────────────────────────────────

    #[test]
    fn object_emits_nested_object_schema() {
        let input: Input<()> = parse(
            r#"{"type":"object","name":"cfg","fields":[{"type":"boolean","name":"debug"}]}"#,
        );
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["cfg"];
        assert_eq!(prop["type"], "object");
        assert!(
            prop["properties"]["debug"].is_object(),
            "nested properties missing"
        );
        let required = prop["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "debug"));
    }

    // ── to_json_schema: allowed-value constraints ────────────────────────────

    #[test]
    fn string_with_allowed_emits_enum_array() {
        let input: Input<()> = parse(
            r#"{"type":"string","name":"env","allowed":["dev","staging","prod"]}"#,
        );
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["env"];
        let enum_arr = prop["enum"].as_array().expect("expected enum array");
        assert_eq!(
            enum_arr,
            &[
                serde_json::json!("dev"),
                serde_json::json!("staging"),
                serde_json::json!("prod")
            ]
        );
    }

    #[test]
    fn integer_with_allowed_emits_enum_array() {
        let input: Input<()> = parse(
            r#"{"type":"integer","name":"port","allowed":[80,443,8080]}"#,
        );
        let schema = to_json_schema(&[input]);
        let enum_arr = schema["properties"]["port"]["enum"]
            .as_array()
            .expect("expected enum array");
        assert_eq!(enum_arr.len(), 3);
        assert!(enum_arr.iter().any(|v| v == &serde_json::json!(80)));
    }

    #[test]
    fn float_with_allowed_emits_enum_array() {
        let input: Input<()> = parse(
            r#"{"type":"float","name":"ratio","allowed":[0.25,0.5,1.0]}"#,
        );
        let schema = to_json_schema(&[input]);
        let enum_arr = schema["properties"]["ratio"]["enum"]
            .as_array()
            .expect("expected enum array");
        assert_eq!(enum_arr.len(), 3);
    }

    #[test]
    fn string_with_described_allowed_emits_one_of() {
        let input: Input<()> = parse(
            r#"{"type":"string","name":"license","allowed":[
                {"value":"mit","description":"MIT License"},
                {"value":"apache"}
            ]}"#,
        );
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["license"];
        let one_of = prop["oneOf"].as_array().expect("expected oneOf");
        assert_eq!(one_of.len(), 2);
        assert_eq!(one_of[0]["const"], "mit");
        assert_eq!(one_of[0]["description"], "MIT License");
        assert_eq!(one_of[1]["const"], "apache");
        assert!(
            one_of[1].get("description").is_none()
                || one_of[1]["description"].is_null()
        );
    }

    // ── to_json_schema: secret flag ──────────────────────────────────────────

    #[test]
    fn secret_string_emits_write_only_and_password_format() {
        let input: Input<()> =
            parse(r#"{"type":"string","name":"token","secret":true}"#);
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["token"];
        assert_eq!(prop["writeOnly"], true);
        assert_eq!(prop["format"], "password");
    }

    #[test]
    fn secret_integer_emits_write_only() {
        let input: Input<()> =
            parse(r#"{"type":"integer","name":"pin","secret":true}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["pin"]["writeOnly"], true);
    }

    #[test]
    fn secret_float_emits_write_only() {
        let input: Input<()> =
            parse(r#"{"type":"float","name":"coeff","secret":true}"#);
        let schema = to_json_schema(&[input]);
        assert_eq!(schema["properties"]["coeff"]["writeOnly"], true);
    }

    #[test]
    fn secret_object_emits_write_only_at_object_level() {
        let input: Input<()> = parse(
            r#"{"type":"object","name":"creds","secret":true,"fields":[{"type":"string","name":"user"}]}"#,
        );
        let schema = to_json_schema(&[input]);
        let prop = &schema["properties"]["creds"];
        assert_eq!(
            prop["writeOnly"], true,
            "expected writeOnly on secret object"
        );
        assert_eq!(prop["type"], "object");
    }

    // ── to_json_schema: required list ────────────────────────────────────────

    #[test]
    fn always_hidden_input_excluded_from_required() {
        let input: Input<()> =
            parse(r#"{"type":"string","name":"hidden","if":false}"#);
        let schema = to_json_schema(&[input]);
        let required = schema["required"].as_array().unwrap();
        assert!(
            !required.iter().any(|v| v == "hidden"),
            "always-hidden input must not appear in required"
        );
    }

    #[test]
    fn input_with_default_excluded_from_required() {
        let input: Input<()> =
            parse(r#"{"type":"boolean","name":"debug","default":false}"#);
        let schema = to_json_schema(&[input]);
        let required = schema["required"].as_array().unwrap();
        assert!(
            !required.iter().any(|v| v == "debug"),
            "input with default must not appear in required"
        );
    }

    // ── JsonSchema trait impl: SUPPORTED gates variant arms ──────────────────

    #[test]
    fn json_schema_excludes_object_arm_when_not_in_supported() {
        use schemars::generate::SchemaSettings;

        #[derive(Debug, Clone, PartialEq, Default)]
        struct NoObjectProfile;

        impl InputProfile for NoObjectProfile {
            const SUPPORTED: EnumSet<InputKind> = enumset::enum_set!(
                InputKind::Boolean
                    | InputKind::String
                    | InputKind::Integer
                    | InputKind::Float
                    | InputKind::StringArray
                    | InputKind::IntegerArray
                    | InputKind::FloatArray
            );
            type Base = ();
            type Boolean = ();
            type String = ();
            type Numeric = ();
            type Array = ();
            type Object = ();
            type AllowedValueBase = ();
        }

        let mut generator = SchemaSettings::default().into_generator();
        let schema =
            <Input<NoObjectProfile> as schemars::JsonSchema>::json_schema(
                &mut generator,
            );
        let value = serde_json::to_value(&schema).unwrap();

        let one_of = value["oneOf"]
            .as_array()
            .expect("expected oneOf at root of Input schema");
        assert_eq!(
            one_of.len(),
            7,
            "expected 7 arms (all variants except Object)"
        );

        let has_object_arm = one_of.iter().any(|arm| {
            arm["allOf"]
                .as_array()
                .and_then(|all_of| all_of.first())
                .and_then(|first| first["properties"]["type"]["const"].as_str())
                .map(|t| t == "object")
                .unwrap_or(false)
        });
        assert!(
            !has_object_arm,
            "Object arm must be excluded when not in SUPPORTED"
        );
    }

    // ── snapshot test: complex fixture ───────────────────────────────────────

    #[test]
    fn complex_fixture_snapshot() {
        let inputs: Vec<Input<()>> = serde_json::from_str(
            r#"[
            {"type":"boolean","name":"debug","default":false},
            {"type":"string","name":"env","allowed":["dev","prod"]},
            {"type":"string","name":"api_key","secret":true},
            {"type":"integer","name":"port","default":8080},
            {"type":"string-array","name":"tags"},
            {"type":"object","name":"tls","fields":[
                {"type":"string","name":"cert"},
                {"type":"string","name":"key","secret":true}
            ]},
            {"type":"float","name":"ratio","allowed":[
                {"value":0.5,"description":"Half"},
                {"value":1.0,"description":"Full"}
            ]}
        ]"#,
        )
        .unwrap();

        let schema = to_json_schema(&inputs);

        let expected = serde_json::json!({
            "type": "object",
            "properties": {
                "debug":   { "type": "boolean" },
                "env":     { "type": "string", "enum": ["dev", "prod"] },
                "api_key": { "type": "string", "writeOnly": true, "format": "password" },
                "port":    { "type": "integer" },
                "tags":    { "type": "array", "items": { "type": "string" } },
                "tls": {
                    "type": "object",
                    "properties": {
                        "cert": { "type": "string" },
                        "key":  { "type": "string", "writeOnly": true, "format": "password" }
                    },
                    "required": ["cert", "key"]
                },
                "ratio": {
                    "type": "number",
                    "oneOf": [
                        { "const": 0.5, "description": "Half" },
                        { "const": 1.0, "description": "Full" }
                    ]
                }
            },
            "required": ["env", "api_key", "tags", "tls", "ratio"]
        });

        assert_eq!(schema, expected);
    }
}
