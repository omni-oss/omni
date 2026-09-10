use std::borrow::Cow;

use omni_projection_configurations::{Projection, ProjectionExtra};
use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as _,
    ser::SerializeStruct,
};
use serde_json::{Value, json};

use crate::SourceConfig;

/// A `projection.omni.{yaml,yml,json,toml}` shipped by a source repository.
///
/// A manifest is either a leaf that declares the routes it recommends for
/// consumers, or a bundle that references further projection sources. The two
/// shapes are mutually exclusive: exactly one of `routes` or `sources` must be
/// present. The wire form carries no discriminator, so an existing leaf
/// manifest (`{routes: [...]}`) parses unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedProjectionConfiguration {
    Leaf {
        routes: Vec<Projection>,
    },
    Meta {
        sources: Vec<SourceConfig<ProjectionExtra>>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(default)]
    routes: Option<Vec<Projection>>,
    #[serde(default)]
    sources: Option<Vec<SourceConfig<ProjectionExtra>>>,
}

impl TryFrom<RawManifest> for OwnedProjectionConfiguration {
    type Error = &'static str;

    fn try_from(raw: RawManifest) -> Result<Self, Self::Error> {
        match (raw.routes, raw.sources) {
            (Some(routes), None) => Ok(Self::Leaf { routes }),
            (None, Some(sources)) => Ok(Self::Meta { sources }),
            (Some(_), Some(_)) => Err(
                "a projection manifest declares either `routes` or `sources`, not both",
            ),
            (None, None) => Err(
                "a projection manifest must declare exactly one of `routes` or `sources`",
            ),
        }
    }
}

impl<'de> Deserialize<'de> for OwnedProjectionConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawManifest::deserialize(deserializer)?;
        OwnedProjectionConfiguration::try_from(raw).map_err(D::Error::custom)
    }
}

impl Serialize for OwnedProjectionConfiguration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st =
            serializer.serialize_struct("OwnedProjectionConfiguration", 1)?;
        match self {
            Self::Leaf { routes } => st.serialize_field("routes", routes)?,
            Self::Meta { sources } => st.serialize_field("sources", sources)?,
        }
        st.end()
    }
}

impl JsonSchema for OwnedProjectionConfiguration {
    fn schema_name() -> Cow<'static, str> {
        "OwnedProjectionConfiguration".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let projection = subschema::<Projection>(generator);
        let source = subschema::<SourceConfig<ProjectionExtra>>(generator);

        let leaf = json!({
            "type": "object",
            "properties": { "routes": { "type": "array", "items": projection } },
            "required": ["routes"],
            "additionalProperties": false
        });
        let meta = json!({
            "type": "object",
            "properties": { "sources": { "type": "array", "items": source } },
            "required": ["sources"],
            "additionalProperties": false
        });

        value_to_schema(json!({ "oneOf": [leaf, meta] }))
    }
}

fn subschema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    serde_json::to_value(generator.subschema_for::<T>())
        .expect("subschema is always valid JSON")
}

fn value_to_schema(value: Value) -> Schema {
    match value {
        Value::Object(map) => Schema::from(map),
        Value::Bool(b) => Schema::from(b),
        _ => panic!("expected a JSON object or bool for Schema"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_manifest_parses_unchanged() {
        let owned: OwnedProjectionConfiguration =
            serde_json::from_str(r#"{"routes":[{"strategy":"namespaced"}]}"#)
                .expect("leaf manifest parses");
        let OwnedProjectionConfiguration::Leaf { routes } = &owned else {
            panic!("expected leaf");
        };
        assert_eq!(routes.len(), 1);

        let re = serde_json::to_string(&owned).unwrap();
        assert!(re.contains("\"routes\""));
        assert!(!re.contains("\"sources\""));
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);
    }

    #[test]
    fn meta_manifest_parses() {
        let owned: OwnedProjectionConfiguration = serde_json::from_str(
            r#"{"sources":[{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"skills"}]}"#,
        )
        .expect("meta manifest parses");
        let OwnedProjectionConfiguration::Meta { sources } = &owned else {
            panic!("expected meta");
        };
        assert_eq!(sources.len(), 1);

        let re = serde_json::to_string(&owned).unwrap();
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);
    }

    #[test]
    fn declaring_both_routes_and_sources_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"routes":[],"sources":[]}"#,
            )
            .is_err(),
            "a manifest with both `routes` and `sources` must be rejected"
        );
    }

    #[test]
    fn declaring_neither_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(r#"{}"#)
                .is_err(),
            "a manifest with neither `routes` nor `sources` must be rejected"
        );
    }

    #[test]
    fn unknown_key_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"routes":[],"typo":1}"#,
            )
            .is_err(),
            "unknown key in a manifest must be rejected"
        );
    }

    #[test]
    fn schema_is_a_one_of_leaf_and_meta() {
        let generator = SchemaGenerator::default();
        let schema = serde_json::to_value(
            generator.into_root_schema_for::<OwnedProjectionConfiguration>(),
        )
        .expect("schema serializes");

        let arms = schema
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("manifest schema is a oneOf");
        assert_eq!(arms.len(), 2, "one arm for leaf, one for meta");

        let has_property = |name: &str| {
            arms.iter().any(|arm| {
                arm.pointer(&format!("/properties/{name}")).is_some()
            })
        };
        assert!(has_property("routes"), "a leaf arm exposes `routes`");
        assert!(has_property("sources"), "a meta arm exposes `sources`");

        for arm in arms {
            assert_eq!(
                arm.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "each arm rejects unknown fields"
            );
        }
    }
}
