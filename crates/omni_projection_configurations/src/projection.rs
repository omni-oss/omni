use omni_config_types::SingleOrMany;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::id_validator::validate_projection_id;
use crate::match_validator::{
    option_validate_match_patterns, validate_match_patterns,
};

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
pub enum ExistingPolicy {
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

/// Whether a `pattern`/`flatten` rule matches files (the default) or whole
/// directories. A directory match links the directory itself as one entry
/// rather than each file beneath it.
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
pub enum MatchKind {
    #[default]
    File,
    Dir,
}

/// Cross-strategy options shared by every projection. Carries no strategy
/// discriminant and no nested `flatten`, so `deny_unknown_fields` is exact:
/// any key not declared here is an unknown field.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectionCommon {
    #[serde(default)]
    pub target: OmniPath,

    #[serde(default)]
    pub on_existing: ExistingPolicy,

    #[serde(default)]
    pub link: LinkKind,

    #[serde(default)]
    pub allow_omni_config: bool,

    #[serde(default)]
    pub allow_git: bool,
}

/// A rule for the `explicit` strategy: a literal source path and a required
/// destination.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExplicitRule {
    pub source: String,

    pub dest: DestPath,
}

/// A rule for the `pattern` strategy: a glob `match`, a required templated
/// `dest`, and an optional directory-vs-file `match_kind`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PatternRule {
    #[serde(rename = "match", deserialize_with = "validate_match_patterns")]
    pub r#match: SingleOrMany<String>,

    pub dest: DestPath,

    #[serde(default)]
    pub match_kind: MatchKind,
}

/// A rule for the `flatten` strategy: a glob `match`, an optional `dest`
/// (defaulting to the entry's basename), and an optional `match_kind`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlattenRule {
    #[serde(rename = "match", deserialize_with = "validate_match_patterns")]
    pub r#match: SingleOrMany<String>,

    #[serde(default)]
    pub dest: Option<DestPath>,

    #[serde(default)]
    pub match_kind: MatchKind,
}

/// Links the whole source tree under `target/<id>` as a single directory link.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamespacedProjection {
    #[serde(flatten)]
    pub common: ProjectionCommon,
}

/// Mirrors every source file one-to-one under `target`, optionally narrowed to
/// a single `scope` glob.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirrorProjection {
    #[serde(flatten)]
    pub common: ProjectionCommon,

    #[serde(default, deserialize_with = "option_validate_match_patterns")]
    pub scope: Option<SingleOrMany<String>>,
}

/// Links literal source paths to explicit destinations.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExplicitProjection {
    #[serde(flatten)]
    pub common: ProjectionCommon,

    pub rules: Vec<ExplicitRule>,
}

/// Routes glob-matched entries to templated destinations, preserving structure.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PatternProjection {
    #[serde(flatten)]
    pub common: ProjectionCommon,

    pub rules: Vec<PatternRule>,
}

/// Routes glob-matched entries to a flat destination directory.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlattenProjection {
    #[serde(flatten)]
    pub common: ProjectionCommon,

    #[serde(default)]
    pub rules: Vec<FlattenRule>,
}

/// How a projection maps source entries onto destinations. Internally tagged by
/// `strategy`; each variant carries only the fields that strategy accepts, so
/// cross-strategy fields are rejected at deserialization time.
///
/// `JsonSchema` is hand-written in `json_schema.rs` — schemars cannot derive an
/// internally-tagged enum whose variants both `flatten` a shared struct and set
/// `additionalProperties: false`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum Projection {
    Namespaced(NamespacedProjection),
    Mirror(MirrorProjection),
    Explicit(ExplicitProjection),
    Pattern(PatternProjection),
    Flatten(FlattenProjection),
}

impl Projection {
    /// The options shared by every strategy.
    pub fn common(&self) -> &ProjectionCommon {
        match self {
            Projection::Namespaced(p) => &p.common,
            Projection::Mirror(p) => &p.common,
            Projection::Explicit(p) => &p.common,
            Projection::Pattern(p) => &p.common,
            Projection::Flatten(p) => &p.common,
        }
    }
}

/// The `extra` family for projection sources: a stable `id` (a relative,
/// `/`-delimited, path-safe namespace) plus optional workspace-declared
/// `routes`. Absent/null `routes` means "inherit the source's owned manifest";
/// present `routes` override it wholesale.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectionExtra {
    #[serde(deserialize_with = "validate_projection_id")]
    pub id: String,

    #[serde(default)]
    pub routes: Option<Vec<Projection>>,
}

/// A `projection.omni.{yaml,yml,json,toml}` shipped by a source repository,
/// declaring the routes it recommends for consumers that do not override them.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnedProjectionConfiguration {
    pub routes: Vec<Projection>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_extra(json: &str) -> Result<ProjectionExtra, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn parse_projection(json: &str) -> Result<Projection, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn target_root_vocabulary_is_workspace_only() {
        parse_projection(r#"{"strategy":"mirror","target":"@workspace/dst"}"#)
            .expect("@workspace target is valid");

        parse_projection(r#"{"strategy":"mirror","target":"dst"}"#)
            .expect("unrooted target is valid");

        assert!(
            parse_projection(
                r#"{"strategy":"mirror","target":"@project/dst"}"#,
            )
            .is_err(),
            "@project target must be rejected"
        );
        assert!(
            parse_projection(r#"{"strategy":"mirror","target":"@home/dst"}"#)
                .is_err(),
            "@home target must be rejected"
        );
    }

    #[test]
    fn dest_root_vocabulary_is_target_only_with_escape() {
        let proj = parse_projection(
            r#"{"strategy":"pattern","rules":[{"match":"**/*","dest":"@target/sub"}]}"#,
        )
        .expect("@target dest is valid");
        let Projection::Pattern(p) = &proj else {
            panic!("expected pattern");
        };
        assert!(p.rules[0].dest.is_rooted(DestRoot::Target));

        parse_projection(
            r#"{"strategy":"pattern","rules":[{"match":"**/*","dest":"sub/thing"}]}"#,
        )
        .expect("unrooted dest is valid");

        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":"**/*","dest":"@workspace/sub"}]}"#,
            )
            .is_err(),
            "@workspace dest must be rejected"
        );

        // `\@myorg/{name}` decodes to a literal-@ first segment (template kept).
        let scoped = parse_projection(
            r#"{"strategy":"flatten","rules":[{"match":"**/*","dest":"\\@myorg/{name}"}]}"#,
        )
        .expect("escaped scope dest is valid");
        let Projection::Flatten(p) = &scoped else {
            panic!("expected flatten");
        };
        let dest = p.rules[0].dest.as_ref().unwrap();
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
                r#"{{"id":"{id}","routes":[{{"strategy":"namespaced"}}]}}"#
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
                r#"{{"id":"{id}","routes":[{{"strategy":"namespaced"}}]}}"#
            );
            assert!(
                parse_extra(&json).is_err(),
                "id `{id}` should be rejected"
            );
        }
    }

    // ── GATING: cross-strategy fields are rejected through the flatten chain ──

    #[test]
    fn rejects_cross_strategy_fields() {
        // `rules` is not valid on namespaced or mirror.
        assert!(
            parse_projection(
                r#"{"strategy":"namespaced","rules":[{"match":"**/*"}]}"#,
            )
            .is_err(),
            "`rules` on namespaced must be rejected"
        );
        assert!(
            parse_projection(
                r#"{"strategy":"mirror","rules":[{"match":"**/*"}]}"#,
            )
            .is_err(),
            "`rules` on mirror must be rejected (scope replaces it)"
        );

        // `scope` is not valid on any strategy but mirror.
        assert!(
            parse_projection(r#"{"strategy":"namespaced","scope":"**/*"}"#)
                .is_err(),
            "`scope` on namespaced must be rejected"
        );

        // `match-kind` is a per-rule field of pattern/flatten only; it is not a
        // projection-level field on any strategy.
        assert!(
            parse_projection(r#"{"strategy":"mirror","match-kind":"dir"}"#,)
                .is_err(),
            "projection-level `match-kind` must be rejected"
        );

        // `match_kind` is not a field of an explicit rule.
        assert!(
            parse_projection(
                r#"{"strategy":"explicit","rules":[{"source":"a","dest":"b","match_kind":"dir"}]}"#,
            )
            .is_err(),
            "`match_kind` on an explicit rule must be rejected"
        );

        // Unknown key on each variant.
        for strategy in
            ["namespaced", "mirror", "explicit", "pattern", "flatten"]
        {
            let json =
                format!(r#"{{"strategy":"{strategy}","totally_unknown":1}}"#);
            assert!(
                parse_projection(&json).is_err(),
                "unknown key on {strategy} must be rejected"
            );
        }

        // Unknown key inside each rule type.
        assert!(
            parse_projection(
                r#"{"strategy":"explicit","rules":[{"source":"a","dest":"b","bogus":1}]}"#,
            )
            .is_err(),
        );
        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":"a","dest":"b","bogus":1}]}"#,
            )
            .is_err(),
        );
        assert!(
            parse_projection(
                r#"{"strategy":"flatten","rules":[{"match":"a","bogus":1}]}"#,
            )
            .is_err(),
        );
    }

    #[test]
    fn round_trips_each_strategy_variant() {
        let cases = [
            r#"{"strategy":"namespaced"}"#,
            r#"{"strategy":"mirror","scope":"**/*.md"}"#,
            r#"{"strategy":"explicit","rules":[{"source":"a.txt","dest":"@target/a.txt"}]}"#,
            r#"{"strategy":"pattern","rules":[{"match":"**/*.md","dest":"{name}.md","match_kind":"file"}]}"#,
            r#"{"strategy":"flatten","rules":[{"match":"**/*","match_kind":"dir"}]}"#,
        ];
        for json in cases {
            let v1: Projection = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("parse {json}: {e}"));
            let re = serde_json::to_string(&v1).expect("serialize");
            let v2: Projection = serde_json::from_str(&re)
                .unwrap_or_else(|e| panic!("re-parse {re}: {e}"));
            assert_eq!(v1, v2, "round-trip mismatch for {json}");
        }
    }

    #[test]
    fn missing_required_dest_is_rejected() {
        assert!(
            parse_projection(
                r#"{"strategy":"explicit","rules":[{"source":"a.txt"}]}"#,
            )
            .is_err(),
            "explicit rule without dest must be rejected"
        );
        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":"a.txt"}]}"#,
            )
            .is_err(),
            "pattern rule without dest must be rejected"
        );
        parse_projection(
            r#"{"strategy":"flatten","rules":[{"match":"a.txt"}]}"#,
        )
        .expect("flatten rule without dest is valid");
    }

    #[test]
    fn match_kind_defaults_to_file_and_parses_variants() {
        let proj = parse_projection(
            r#"{"strategy":"pattern","rules":[{"match":"a","dest":"b"}]}"#,
        )
        .unwrap();
        let Projection::Pattern(p) = &proj else {
            panic!("expected pattern");
        };
        assert_eq!(p.rules[0].match_kind, MatchKind::File);

        for (raw, expected) in
            [("file", MatchKind::File), ("dir", MatchKind::Dir)]
        {
            let json = format!(
                r#"{{"strategy":"pattern","rules":[{{"match":"a","dest":"b","match_kind":"{raw}"}}]}}"#
            );
            let proj = parse_projection(&json).unwrap();
            let Projection::Pattern(p) = &proj else {
                panic!("expected pattern");
            };
            assert_eq!(p.rules[0].match_kind, expected);
        }

        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":"a","dest":"b","match_kind":"tree"}]}"#,
            )
            .is_err(),
            "unknown match_kind must be rejected"
        );
    }

    #[test]
    fn routes_absence_null_and_lists_deserialize() {
        assert_eq!(parse_extra(r#"{"id":"a"}"#).unwrap().routes, None);
        assert_eq!(
            parse_extra(r#"{"id":"a","routes":null}"#).unwrap().routes,
            None
        );
        assert_eq!(
            parse_extra(r#"{"id":"a","routes":[]}"#).unwrap().routes,
            Some(vec![])
        );
        let some =
            parse_extra(r#"{"id":"a","routes":[{"strategy":"namespaced"}]}"#)
                .unwrap();
        assert_eq!(some.routes.as_deref().map(<[_]>::len), Some(1));
    }

    #[test]
    fn owned_configuration_round_trips_and_rejects_unknown_key() {
        let owned: OwnedProjectionConfiguration =
            serde_json::from_str(r#"{"routes":[{"strategy":"namespaced"}]}"#)
                .unwrap();
        assert_eq!(owned.routes.len(), 1);
        let re = serde_json::to_string(&owned).unwrap();
        let back: OwnedProjectionConfiguration =
            serde_json::from_str(&re).unwrap();
        assert_eq!(owned, back);

        assert!(
            serde_json::from_str::<OwnedProjectionConfiguration>(
                r#"{"routes":[],"typo":1}"#,
            )
            .is_err(),
            "unknown key in owned manifest must be rejected"
        );
    }

    #[test]
    fn match_and_scope_accept_string_and_list_forms() {
        let single = parse_projection(
            r#"{"strategy":"pattern","rules":[{"match":"**/*.md","dest":"{name}.md"}]}"#,
        )
        .expect("string match is valid");
        let Projection::Pattern(p) = &single else {
            panic!("expected pattern");
        };
        assert_eq!(
            p.rules[0].r#match,
            SingleOrMany::Single("**/*.md".to_string())
        );

        let list = parse_projection(
            r#"{"strategy":"pattern","rules":[{"match":["**/*.md","!drafts/**"],"dest":"{name}.md"}]}"#,
        )
        .expect("list match is valid");
        let Projection::Pattern(p) = &list else {
            panic!("expected pattern");
        };
        assert_eq!(
            p.rules[0].r#match,
            SingleOrMany::Many(vec![
                "**/*.md".to_string(),
                "!drafts/**".to_string()
            ])
        );

        let brace = parse_projection(
            r#"{"strategy":"flatten","rules":[{"match":"{a,b,c}"}]}"#,
        )
        .expect("brace-glob scalar is valid");
        let Projection::Flatten(p) = &brace else {
            panic!("expected flatten");
        };
        assert_eq!(
            p.rules[0].r#match,
            SingleOrMany::Single("{a,b,c}".to_string())
        );

        let scope_single =
            parse_projection(r#"{"strategy":"mirror","scope":"docs/**"}"#)
                .expect("string scope is valid");
        let Projection::Mirror(m) = &scope_single else {
            panic!("expected mirror");
        };
        assert_eq!(m.scope, Some(SingleOrMany::Single("docs/**".to_string())));

        let scope_list = parse_projection(
            r#"{"strategy":"mirror","scope":["docs/**","!docs/drafts/**"]}"#,
        )
        .expect("list scope is valid");
        let Projection::Mirror(m) = &scope_list else {
            panic!("expected mirror");
        };
        assert_eq!(
            m.scope,
            Some(SingleOrMany::Many(vec![
                "docs/**".to_string(),
                "!docs/drafts/**".to_string()
            ]))
        );
    }

    #[test]
    fn rejects_invalid_match_and_scope_lists() {
        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":[],"dest":"x"}]}"#,
            )
            .is_err(),
            "empty match list must be rejected"
        );
        assert!(
            parse_projection(r#"{"strategy":"mirror","scope":[]}"#).is_err(),
            "empty scope list must be rejected"
        );

        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":["a","  "],"dest":"x"}]}"#,
            )
            .is_err(),
            "whitespace-only match entry must be rejected"
        );
        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":"","dest":"x"}]}"#,
            )
            .is_err(),
            "empty match scalar must be rejected"
        );

        assert!(
            parse_projection(
                r#"{"strategy":"pattern","rules":[{"match":["!a","!b"],"dest":"x"}]}"#,
            )
            .is_err(),
            "exclude-only match list must be rejected"
        );
        assert!(
            parse_projection(r#"{"strategy":"mirror","scope":"!drafts/**"}"#,)
                .is_err(),
            "exclude-only scope scalar must be rejected"
        );

        parse_projection(r#"{"strategy":"mirror"}"#)
            .expect("absent scope is valid");
        parse_projection(r#"{"strategy":"mirror","scope":null}"#)
            .expect("null scope is valid");
    }

    #[test]
    fn explicit_rule_uses_source_not_match() {
        let proj = parse_projection(
            r#"{"strategy":"explicit","rules":[{"source":"a.txt","dest":"@target/a.txt"}]}"#,
        )
        .expect("explicit rule with source is valid");
        let Projection::Explicit(p) = &proj else {
            panic!("expected explicit");
        };
        assert_eq!(p.rules[0].source, "a.txt");

        assert!(
            parse_projection(
                r#"{"strategy":"explicit","rules":[{"match":"a.txt","dest":"@target/a.txt"}]}"#,
            )
            .is_err(),
            "`match` on an explicit rule must be an unknown-field error"
        );
    }

    #[test]
    fn on_existing_replaces_on_collision() {
        parse_projection(r#"{"strategy":"mirror","on_existing":"backup"}"#)
            .expect("on_existing is the accepted key");

        assert!(
            parse_projection(
                r#"{"strategy":"mirror","on_collision":"backup"}"#,
            )
            .is_err(),
            "the retired `on_collision` key must be an unknown-field error"
        );
    }
}
