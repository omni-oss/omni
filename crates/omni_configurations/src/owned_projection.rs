use std::borrow::Cow;

use omni_projection_configurations::Projection;
use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as _,
    ser::SerializeStruct,
};
use serde_json::{Value, json};

use crate::{
    ProjectionProfile, SourceConfig, validators::option_validate_source_name,
};

/// A `projection.omni.{yaml,yml,json,toml}` shipped by a source repository.
///
/// A manifest carries a required author `name`, optional metadata
/// (`version`/`description`), and a body that is either a leaf declaring the
/// routes it recommends for consumers, or a bundle referencing further
/// projection sources. The two body shapes are mutually exclusive: exactly one
/// of `routes` or `sources` must be present. The wire form carries no
/// discriminator between the two bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedProjectionConfiguration {
    /// The author-declared canonical name (`@org/name` or a bare `name`). A
    /// display label and future registry coordinate, never a ledger key.
    pub name: String,
    /// Author metadata; not a version constraint or resolution key.
    pub version: Option<String>,
    pub description: Option<String>,
    pub body: OwnedProjectionBody,
}

/// The mutually-exclusive body of a projection manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedProjectionBody {
    Leaf {
        routes: Vec<Projection>,
    },
    Meta {
        sources: Vec<SourceConfig<ProjectionProfile>>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(default, deserialize_with = "option_validate_source_name")]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    routes: Option<Vec<Projection>>,
    #[serde(default)]
    sources: Option<Vec<SourceConfig<ProjectionProfile>>>,
}

impl TryFrom<RawManifest> for OwnedProjectionConfiguration {
    type Error = &'static str;

    fn try_from(raw: RawManifest) -> Result<Self, Self::Error> {
        let body = match (raw.routes, raw.sources) {
            (Some(routes), None) => OwnedProjectionBody::Leaf { routes },
            (None, Some(sources)) => OwnedProjectionBody::Meta { sources },
            (Some(_), Some(_)) => {
                return Err(
                    "a projection manifest declares either `routes` or `sources`, not both",
                );
            }
            (None, None) => {
                return Err(
                    "a projection manifest must declare exactly one of `routes` or `sources`",
                );
            }
        };

        let name = raw
            .name
            .ok_or("a projection manifest must declare a `name`")?;

        Ok(Self {
            name,
            version: raw.version,
            description: raw.description,
            body,
        })
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
        let field_count = 2
            + self.version.is_some() as usize
            + self.description.is_some() as usize;
        let mut st = serializer
            .serialize_struct("OwnedProjectionConfiguration", field_count)?;
        st.serialize_field("name", &self.name)?;
        if let Some(version) = &self.version {
            st.serialize_field("version", version)?;
        }
        if let Some(description) = &self.description {
            st.serialize_field("description", description)?;
        }
        match &self.body {
            OwnedProjectionBody::Leaf { routes } => {
                st.serialize_field("routes", routes)?
            }
            OwnedProjectionBody::Meta { sources } => {
                st.serialize_field("sources", sources)?
            }
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
        let source = subschema::<SourceConfig<ProjectionProfile>>(generator);

        let metadata = json!({
            "name": { "type": "string" },
            "version": { "type": "string" },
            "description": { "type": "string" },
        });

        let mut leaf_properties = metadata.clone();
        leaf_properties["routes"] =
            json!({ "type": "array", "items": projection });
        let leaf = json!({
            "type": "object",
            "properties": leaf_properties,
            "required": ["name", "routes"],
            "additionalProperties": false
        });

        let mut meta_properties = metadata;
        meta_properties["sources"] =
            json!({ "type": "array", "items": source });
        let meta = json!({
            "type": "object",
            "properties": meta_properties,
            "required": ["name", "sources"],
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
    fn leaf_manifest_requires_name() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"routes":[{"strategy":"namespaced"}]}"#,
            )
            .is_err(),
            "a manifest without a `name` must be rejected"
        );
    }

    #[test]
    fn named_leaf_manifest_round_trips() {
        let owned: OwnedProjectionConfiguration = serde_json::from_str(
            r#"{"name":"@org/skills","routes":[{"strategy":"namespaced"}]}"#,
        )
        .expect("named leaf manifest parses");
        assert_eq!(owned.name, "@org/skills");
        let OwnedProjectionBody::Leaf { routes } = &owned.body else {
            panic!("expected leaf");
        };
        assert_eq!(routes.len(), 1);

        let re = serde_json::to_string(&owned).unwrap();
        assert!(re.contains("\"routes\""));
        assert!(!re.contains("\"sources\""));
        assert!(re.contains("\"name\""));
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);
    }

    #[test]
    fn meta_manifest_parses() {
        let owned: OwnedProjectionConfiguration = serde_json::from_str(
            r#"{"name":"@org/skills","sources":[{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"skills"}]}"#,
        )
        .expect("meta manifest parses");
        let OwnedProjectionBody::Meta { sources } = &owned.body else {
            panic!("expected meta");
        };
        assert_eq!(sources.len(), 1);

        let re = serde_json::to_string(&owned).unwrap();
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);
    }

    #[test]
    fn manifest_with_name_version_description_round_trips() {
        let owned: OwnedProjectionConfiguration = serde_json::from_str(
            r#"{"name":"@org/skills","version":"1.0.0","description":"House skills","routes":[{"strategy":"namespaced"}]}"#,
        )
        .expect("manifest with metadata parses");
        assert_eq!(owned.name, "@org/skills");
        assert_eq!(owned.version.as_deref(), Some("1.0.0"));
        assert_eq!(owned.description.as_deref(), Some("House skills"));

        let re = serde_json::to_string(&owned).unwrap();
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);
    }

    #[test]
    fn invalid_name_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"name":"not a valid name","routes":[]}"#,
            )
            .is_err(),
            "a name that violates the source-name rule must be rejected"
        );
    }

    #[test]
    fn declaring_both_routes_and_sources_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"name":"@org/x","routes":[],"sources":[]}"#,
            )
            .is_err(),
            "a manifest with both `routes` and `sources` must be rejected"
        );
    }

    #[test]
    fn declaring_neither_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"name":"@org/x"}"#
            )
            .is_err(),
            "a manifest with neither `routes` nor `sources` must be rejected"
        );
    }

    #[test]
    fn unknown_key_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"name":"@org/x","routes":[],"typo":1}"#,
            )
            .is_err(),
            "unknown key in a manifest must be rejected"
        );
    }

    #[test]
    fn ambient_key_is_rejected() {
        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"name":"@org/x","routes":[],"env":{}}"#,
            )
            .is_err(),
            "an ambient key in a manifest must be rejected"
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
        assert!(has_property("name"), "arms expose the required `name`");

        for arm in arms {
            let required = arm
                .pointer("/required")
                .and_then(Value::as_array)
                .expect("each arm lists required fields");
            assert!(
                required.contains(&Value::String("name".to_string())),
                "each arm requires `name`"
            );
            assert_eq!(
                arm.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "each arm rejects unknown fields"
            );
        }
    }
}
