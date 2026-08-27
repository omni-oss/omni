use garde::Validate;
use omni_config_types::SingleOrMany;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

/// A registered source of configuration artifacts (generator manifests, tool
/// manifests, or projection inputs).
///
/// The type is generic over an `extra` family `E` that is flattened into each
/// variant. `E = NoExtra` yields the plain `local`/`git` source shared by
/// generators and tools; richer families (e.g. projections) add sibling fields
/// without changing the wire shape of the `source`/`path`/`uri`/`rev` keys.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    deny_unknown_fields,
    bound(deserialize = "E: Deserialize<'de>", serialize = "E: Serialize")
)]
#[schemars(bound = "E: JsonSchema")]
#[garde(allow_unvalidated)]
pub enum SourceConfig<E = NoExtra> {
    Local(LocalSource<E>),
    Git(GitSource<E>),
}

/// A `local` source: one or more workspace-relative paths.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "E: Deserialize<'de>", serialize = "E: Serialize")
)]
#[schemars(bound = "E: JsonSchema")]
#[garde(allow_unvalidated)]
pub struct LocalSource<E = NoExtra> {
    pub path: SingleOrMany<String>,

    #[serde(flatten)]
    #[garde(skip)]
    pub extra: E,
}

/// A `git` source: a repository URI pinned to a revision.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(
    deny_unknown_fields,
    bound(deserialize = "E: Deserialize<'de>", serialize = "E: Serialize")
)]
#[schemars(bound = "E: JsonSchema")]
#[garde(allow_unvalidated)]
pub struct GitSource<E = NoExtra> {
    pub uri: Url,

    pub rev: String,

    #[serde(flatten)]
    #[garde(skip)]
    pub extra: E,
}

/// The empty `extra` family: a source with no additional fields.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq,
)]
#[serde(deny_unknown_fields)]
pub struct NoExtra {}

/// Generator sources have no extra fields.
pub type GeneratorSourceConfiguration = SourceConfig<NoExtra>;

/// Tool sources have no extra fields; the serialized shape is identical to
/// [`GeneratorSourceConfiguration`].
pub type ToolSourceConfiguration = SourceConfig<NoExtra>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_source_round_trips_legacy_wire_shape() {
        let json = r#"{"source":"local","path":"./tools"}"#;
        let parsed: SourceConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(
            parsed,
            SourceConfig::Local(LocalSource {
                path: SingleOrMany::Single("./tools".to_string()),
                extra: NoExtra {},
            })
        );
        assert_eq!(serde_json::to_string(&parsed).expect("serialize"), json);
    }

    #[test]
    fn git_source_round_trips_legacy_wire_shape() {
        let json = r#"{"source":"git","uri":"https://example.com/a.git","rev":"main"}"#;
        let parsed: SourceConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(serde_json::to_string(&parsed).expect("serialize"), json);
    }

    #[test]
    fn git_source_rejects_unknown_key() {
        let json = r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","typo":1}"#;
        let result = serde_json::from_str::<SourceConfig>(json);
        assert!(result.is_err(), "unknown key must be rejected");
    }

    #[test]
    fn local_source_rejects_unknown_key() {
        let json = r#"{"source":"local","path":"./x","typo":1}"#;
        let result = serde_json::from_str::<SourceConfig>(json);
        assert!(result.is_err(), "unknown key must be rejected");
    }
}
