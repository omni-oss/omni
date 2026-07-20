//! Manual [`JsonSchema`] impls for the capability config types.
//!
//! Only **one** thing about these types resists `derive(JsonSchema)`: the set
//! of valid [`domain`](crate::CapabilityDomain)s depends on the *profile* `P`
//! ([`CapabilityProfile::SUPPORTED`]), so the emitted `domain` enum must be
//! filtered per profile — matching what [`validate`](crate::validate) enforces
//! at load. A plain derive would always list every domain (e.g. `net` for the
//! generator), diverging from the validator.
//!
//! To keep the schema from drifting out of sync with the Rust types, this impl
//! does **not** re-list the rule's fields by hand. It starts from the *derived*
//! [`CapabilityRule`] schema — so every rule field (`access`, `patterns`,
//! `on_unenforceable`, and anything added later) is picked up automatically —
//! then overrides just the `domain` property with the profile-filtered enum and
//! attaches the profile's `applies_to` selector and flattened `extra`. Adding a
//! field to [`CapabilityRule`] therefore needs no change here.

use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde_json::{Value, json};

use crate::{Capability, CapabilityProfile, CapabilityRule, CapabilityRules};

fn value_to_schema(v: Value) -> Schema {
    match v {
        Value::Object(map) => Schema::from(map),
        Value::Bool(b) => Schema::from(b),
        _ => panic!("expected JSON object or bool for Schema"),
    }
}

fn subschema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    serde_json::to_value(generator.subschema_for::<T>())
        .expect("a Schema is always valid JSON")
}

/// The **inline** (non-`$ref`) object schema for `T`, as a mutable JSON value.
///
/// Unlike [`subschema`], this returns `T`'s definition itself rather than a
/// reference into `$defs`, so callers can override individual properties.
fn inline_schema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    serde_json::to_value(T::json_schema(generator))
        .expect("a Schema is always valid JSON")
}

/// Insert (or replace) a property on an object schema's `properties` map.
fn set_property(schema: &mut Value, name: &str, value: Value) {
    let props = schema
        .as_object_mut()
        .expect("an object schema")
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("`properties` is an object");
    props.insert(name.to_string(), value);
}

impl<P: CapabilityProfile> JsonSchema for Capability<P> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Capability_{}", P::NAME).into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        // Start from the derived rule schema so every `CapabilityRule` field is
        // reflected automatically. Only `domain` needs correcting.
        let mut rule = inline_schema::<CapabilityRule>(generator);

        // The one thing the derive can't express: restrict `domain` to the
        // profile's supported set, so the schema matches `validate` (which
        // rejects unsupported domains at load).
        let domains: Vec<Value> = P::SUPPORTED
            .iter()
            .map(|d| Value::String(d.to_string()))
            .collect();
        set_property(
            &mut rule,
            "domain",
            json!({
                "description": "The capability domain this rule governs.",
                "enum": domains
            }),
        );

        // The profile's `applies_to` selector is optional (`#[serde(default)]`),
        // so it is added as a property but never marked required.
        set_property(
            &mut rule,
            "applies_to",
            subschema::<P::AppliesTo>(generator),
        );

        // Fold in the profile's flattened per-entry extras. For profiles with no
        // extras (`NoExtra`) this is an empty object schema and adds nothing.
        let extra = subschema::<P::Extra>(generator);
        value_to_schema(json!({ "allOf": [rule, extra] }))
    }
}

impl<P: CapabilityProfile> JsonSchema for CapabilityRules<P> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("CapabilityRules_{}", P::NAME).into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = subschema::<Capability<P>>(generator);
        value_to_schema(json!({
            "description": "An ordered list of allow/deny capability rules. Levels cascade by concatenation; a matching deny always wins.",
            "type": "array",
            "items": item,
            "default": []
        }))
    }
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use schemars::generate::SchemaGenerator;
    use serde_json::Value;

    use crate::{CapabilityProfile, CapabilityRule, CapabilityRules};

    // A local profile restricting the supported domains, to prove the emitted
    // `domain` enum is filtered by `SUPPORTED`.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    struct FsOnly;

    impl CapabilityProfile for FsOnly {
        const SUPPORTED: &'static [crate::CapabilityDomain] = &[
            crate::CapabilityDomain::FsRead,
            crate::CapabilityDomain::FsWrite,
        ];
        const NAME: &'static str = "fs-only";
        type AppliesTo = crate::NoExtra;
        type Extra = crate::NoExtra;
        type Context = ();
    }

    fn domain_enum<P: CapabilityProfile>() -> Vec<String> {
        let generator = SchemaGenerator::default();
        let root = generator.into_root_schema_for::<CapabilityRules<P>>();
        // Walk the generated JSON to find the `domain` enum values, resolving
        // through `$defs` as needed.
        let root = serde_json::to_value(&root).expect("valid json");
        let defs = root.get("$defs").cloned().unwrap_or(Value::Null);
        find_domain_enum(&root, &defs).expect("domain enum present")
    }

    fn find_domain_enum(node: &Value, defs: &Value) -> Option<Vec<String>> {
        match node {
            Value::Object(map) => {
                if let Some(Value::Object(props)) = map.get("properties")
                    && let Some(domain) = props.get("domain")
                    && let Some(Value::Array(vals)) = domain.get("enum")
                {
                    return Some(
                        vals.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    );
                }
                if let Some(Value::String(reference)) = map.get("$ref")
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                    && let Some(def) = defs.get(name)
                {
                    return find_domain_enum(def, defs);
                }
                for v in map.values() {
                    if let Some(found) = find_domain_enum(v, defs) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(items) => {
                items.iter().find_map(|v| find_domain_enum(v, defs))
            }
            _ => None,
        }
    }

    #[test]
    fn domain_enum_is_filtered_to_supported() {
        let domains = domain_enum::<FsOnly>();
        assert!(domains.contains(&"fs.read".to_string()));
        assert!(domains.contains(&"fs.write".to_string()));
        assert!(
            !domains.contains(&"net".to_string()),
            "unsupported `net` must be absent, got {domains:?}"
        );
        assert!(!domains.contains(&"process".to_string()));
    }

    #[test]
    fn rule_schema_exposes_on_unenforceable() {
        // `CapabilityRule::on_unenforceable` is a real, deserializable field, so
        // the emitted schema must offer it (optional) and describe its variants
        // via the shared `UnenforceablePolicy` definition.
        let generator = SchemaGenerator::default();
        let root = generator.into_root_schema_for::<CapabilityRules<FsOnly>>();
        let root = serde_json::to_value(&root).expect("valid json");
        let defs = root.get("$defs").cloned().unwrap_or(Value::Null);

        let prop = find_property(&root, "on_unenforceable").expect(
            "the `on_unenforceable` property must appear in the schema",
        );

        // Resolve the property (a `$ref` into `$defs`) and collect the string
        // variants, whether emitted as a flat `enum` or as `oneOf`/`const`s.
        let mut variants = Vec::new();
        collect_string_variants(prop, &defs, &mut variants);
        assert!(variants.contains(&"allow".to_string()));
        assert!(variants.contains(&"warn".to_string()));
        assert!(variants.contains(&"deny".to_string()));
    }

    #[test]
    fn every_rule_field_is_projected() {
        // Drift guard: the profile-projected `Capability` schema must expose
        // every field of the derived `CapabilityRule` schema. If a field is
        // added to `CapabilityRule`, this fails unless it flows through here
        // (which it does automatically, since we build from the derived rule).
        let mut g = SchemaGenerator::default();
        let rule = serde_json::to_value(CapabilityRule::json_schema(&mut g))
            .expect("valid json");
        let rule_fields: Vec<String> = rule["properties"]
            .as_object()
            .expect("rule has properties")
            .keys()
            .cloned()
            .collect();
        assert!(
            rule_fields.iter().any(|f| f == "on_unenforceable"),
            "sanity: the derived rule should carry on_unenforceable"
        );

        let cap =
            serde_json::to_value(super::Capability::<FsOnly>::json_schema(
                &mut SchemaGenerator::default(),
            ))
            .expect("valid json");
        for field in rule_fields {
            assert!(
                find_property(&cap, &field).is_some(),
                "rule field `{field}` is missing from the projected Capability schema"
            );
        }
    }

    fn find_property<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
        match node {
            Value::Object(map) => {
                if let Some(Value::Object(props)) = map.get("properties")
                    && let Some(found) = props.get(name)
                {
                    return Some(found);
                }
                map.values().find_map(|v| find_property(v, name))
            }
            Value::Array(items) => {
                items.iter().find_map(|v| find_property(v, name))
            }
            _ => None,
        }
    }

    fn collect_string_variants(
        node: &Value,
        defs: &Value,
        out: &mut Vec<String>,
    ) {
        match node {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get("$ref")
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                    && let Some(def) = defs.get(name)
                {
                    collect_string_variants(def, defs, out);
                }
                if let Some(Value::String(c)) = map.get("const") {
                    out.push(c.clone());
                }
                if let Some(Value::Array(vals)) = map.get("enum") {
                    out.extend(
                        vals.iter()
                            .filter_map(|v| v.as_str().map(String::from)),
                    );
                }
                for v in map.values() {
                    collect_string_variants(v, defs, out);
                }
            }
            Value::Array(items) => {
                for v in items {
                    collect_string_variants(v, defs, out);
                }
            }
            _ => {}
        }
    }
}
