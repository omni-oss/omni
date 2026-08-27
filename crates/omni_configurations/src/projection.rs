use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::SourceConfig;
use crate::validators::validate_projection_id;

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

pub type ProjectionSourceConfiguration = SourceConfig<ProjectionExtra>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkspaceConfiguration;

    fn parse_source(
        json: &str,
    ) -> Result<ProjectionSourceConfiguration, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn parse_workspace(
        json: &str,
    ) -> Result<WorkspaceConfiguration, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn parses_git_and_local_projection_sources() {
        let git = parse_source(
            r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"team-ai-skills","projections":[{"strategy":"namespaced"}]}"#,
        )
        .expect("valid git projection source");
        match git {
            SourceConfig::Git(g) => {
                assert_eq!(g.extra.id, "team-ai-skills");
                assert_eq!(g.extra.projections.len(), 1);
            }
            SourceConfig::Local(_) => panic!("expected git"),
        }

        let local = parse_source(
            r#"{"source":"local","path":"./vendor","id":"shared_scripts","projections":[{"strategy":"flatten","rules":[{"match":"**/*.sh"}]}]}"#,
        )
        .expect("valid local projection source");
        match local {
            SourceConfig::Local(l) => {
                assert_eq!(l.extra.id, "shared_scripts");
            }
            SourceConfig::Git(_) => panic!("expected local"),
        }
    }

    #[test]
    fn target_root_vocabulary_is_workspace_only() {
        parse_source(
            r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"mirror","target":"@workspace/dst"}]}"#,
        )
        .expect("@workspace target is valid");

        parse_source(
            r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"mirror","target":"dst"}]}"#,
        )
        .expect("unrooted target is valid");

        assert!(
            parse_source(
                r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"mirror","target":"@project/dst"}]}"#,
            )
            .is_err(),
            "@project target must be rejected"
        );
        assert!(
            parse_source(
                r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"mirror","target":"@home/dst"}]}"#,
            )
            .is_err(),
            "@home target must be rejected"
        );
    }

    #[test]
    fn dest_root_vocabulary_is_target_only_with_escape() {
        let source = parse_source(
            r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"@target/sub"}]}]}"#,
        )
        .expect("@target dest is valid");
        let SourceConfig::Local(local) = source else {
            panic!("expected local");
        };
        let dest = local.extra.projections[0].rules[0].dest.as_ref().unwrap();
        assert!(dest.is_rooted(DestRoot::Target));

        // Unrooted dest is target-relative.
        parse_source(
            r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"sub/thing"}]}]}"#,
        )
        .expect("unrooted dest is valid");

        // A non-Target root is rejected.
        assert!(
            parse_source(
                r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"pattern","rules":[{"match":"**/*","dest":"@workspace/sub"}]}]}"#,
            )
            .is_err(),
            "@workspace dest must be rejected"
        );

        // `\@myorg/{name}` decodes to a literal-@ first segment (template kept).
        let scoped = parse_source(
            r#"{"source":"local","path":"./v","id":"a","projections":[{"strategy":"flatten","rules":[{"match":"**/*","dest":"\\@myorg/{name}"}]}]}"#,
        )
        .expect("escaped scope dest is valid");
        let SourceConfig::Local(local) = scoped else {
            panic!("expected local");
        };
        let dest = local.extra.projections[0].rules[0].dest.as_ref().unwrap();
        assert!(!dest.is_any_rooted());
        assert_eq!(
            dest.unresolved_path(),
            std::path::Path::new("@myorg/{name}")
        );
    }

    #[test]
    fn projection_source_rejects_unknown_key() {
        assert!(
            parse_source(
                r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"a","projections":[],"typo":1}"#,
            )
            .is_err(),
            "unknown key under a projection source must be rejected"
        );
    }

    #[test]
    fn workspace_rejects_duplicate_projection_id() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"projections":[{"source":"local","path":"./a","id":"dup","projections":[{"strategy":"namespaced"}]},{"source":"local","path":"./b","id":"dup","projections":[{"strategy":"namespaced"}]}]}"#,
            )
            .is_err(),
            "duplicate projection id must be rejected"
        );
    }

    #[test]
    fn workspace_rejects_rules_on_namespaced_strategy() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"projections":[{"source":"local","path":"./a","id":"a","projections":[{"strategy":"namespaced","rules":[{"match":"**/*"}]}]}]}"#,
            )
            .is_err(),
            "`rules` on a namespaced strategy must be rejected"
        );
    }

    #[test]
    fn projection_id_accepts_safe_values() {
        for id in ["team-ai-skills", "shared_scripts", "a", "@myorg/pkg"] {
            let json = format!(
                r#"{{"source":"local","path":"./v","id":"{id}","projections":[{{"strategy":"namespaced"}}]}}"#
            );
            parse_source(&json)
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
                r#"{{"source":"local","path":"./v","id":"{id}","projections":[{{"strategy":"namespaced"}}]}}"#
            );
            assert!(
                parse_source(&json).is_err(),
                "id `{id}` should be rejected"
            );
        }
    }
}
