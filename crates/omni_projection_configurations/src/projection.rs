use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::id_validator::validate_projection_id;

/// Root vocabulary for a projection `target`. `@workspace/...` and unrooted
/// targets both resolve against the workspace root.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    enum_map::Enum,
    strum::Display,
    strum::VariantArray,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Root {
    Workspace,
}

pub type OmniPath = omni_types::OmniPath<Root>;

/// Root vocabulary for a rule `dest`. `@target/...` and unrooted dests both
/// resolve against the projection's already-resolved `target`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    enum_map::Enum,
    strum::Display,
    strum::VariantArray,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum DestRoot {
    Target,
}

pub type DestPath = omni_types::OmniPath<DestRoot>;

/// How a projection maps source entries onto destinations.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionStrategy {
    Mirror,
    Explicit,
    Pattern,
    Flatten,
    Namespaced,
}

/// What to do when a destination already exists.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum CollisionPolicy {
    Skip,
    Overwrite,
    #[default]
    Backup,
    Error,
}

/// The kind of link a projection materializes.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    #[default]
    Auto,
    Symlink,
    Junction,
    Hardlink,
    Copy,
}

/// A single routing rule: a glob `match` and an optional destination.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRule {
    #[serde(rename = "match")]
    pub r#match: String,

    #[serde(default)]
    pub dest: Option<DestPath>,
}

/// One projection within a source: a strategy, a destination anchor, and the
/// rules that drive it.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub strategy: ProjectionStrategy,

    #[serde(default)]
    pub target: OmniPath,

    #[serde(default)]
    pub rules: Vec<ProjectionRule>,

    #[serde(default)]
    pub on_collision: CollisionPolicy,

    #[serde(default)]
    pub link: LinkKind,

    #[serde(default)]
    pub allow_omni_config: bool,

    #[serde(default)]
    pub allow_git: bool,
}

/// The `extra` family for projection sources: a stable `id` (a relative,
/// `/`-delimited, path-safe namespace) plus the source's projections.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectionExtra {
    #[serde(deserialize_with = "validate_projection_id")]
    pub id: String,

    pub projections: Vec<Projection>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_extra(
        json: &str,
    ) -> Result<ProjectionExtra, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn target_root_vocabulary_is_workspace_only() {
        parse_extra(
            r#"{"id":"a","projections":[{"strategy":"mirror","target":"@workspace/dst"}]}"#,
        )
        .expect("@workspace target is valid");

        parse_extra(
            r#"{"id":"a","projections":[{"strategy":"mirror","target":"dst"}]}"#,
        )
        .expect("unrooted target is valid");

        assert!(
            parse_extra(
                r#"{"id":"a","projections":[{"strategy":"mirror","target":"@project/dst"}]}"#,
            )
            .is_err(),
            "@project target must be rejected"
        );
        assert!(
            parse_extra(
                r#"{"id":"a","projections":[{"strategy":"mirror","target":"@home/dst"}]}"#,
            )
            .is_err(),
            "@home target must be rejected"
        );
    }

    #[test]
    fn dest_root_vocabulary_is_target_only_with_escape() {
        let extra = parse_extra(
            r#"{"id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"@target/sub"}]}]}"#,
        )
        .expect("@target dest is valid");
        let dest = extra.projections[0].rules[0].dest.as_ref().unwrap();
        assert!(dest.is_rooted(DestRoot::Target));

        parse_extra(
            r#"{"id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"sub/thing"}]}]}"#,
        )
        .expect("unrooted dest is valid");

        assert!(
            parse_extra(
                r#"{"id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"@workspace/sub"}]}]}"#,
            )
            .is_err(),
            "@workspace dest must be rejected"
        );

        // `\@myorg/{name}` decodes to a literal-@ first segment (template kept).
        let scoped = parse_extra(
            r#"{"id":"a","projections":[{"strategy":"flatten","rules":[{"match":"**/*","dest":"\\@myorg/{name}"}]}]}"#,
        )
        .expect("escaped scope dest is valid");
        let dest = scoped.projections[0].rules[0].dest.as_ref().unwrap();
        assert!(!dest.is_any_rooted());
        assert_eq!(
            dest.unresolved_path(),
            std::path::Path::new("@myorg/{name}")
        );
    }

    #[test]
    fn projection_id_accepts_safe_values() {
        for id in ["team-ai-skills", "shared_scripts", "a", "@myorg/pkg"] {
            let json = format!(
                r#"{{"id":"{id}","projections":[{{"strategy":"namespaced"}}]}}"#
            );
            parse_extra(&json)
                .unwrap_or_else(|e| panic!("id `{id}` should be valid: {e}"));
        }
    }

    #[test]
    fn projection_id_rejects_unsafe_values() {
        for id in [
            "a\\b", "..", ".hidden", "abc.", "con", "CON", "a<b>", "a:b",
            "a|b", "a?b", "a*b", "", "//a", "/abs", "a/../b",
        ] {
            let json = format!(
                r#"{{"id":"{id}","projections":[{{"strategy":"namespaced"}}]}}"#
            );
            assert!(
                parse_extra(&json).is_err(),
                "id `{id}` should be rejected"
            );
        }
    }
}
