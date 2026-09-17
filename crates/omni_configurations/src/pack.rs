use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    GeneratorSourceConfiguration, ProjectionSourceConfiguration, SourceConfig,
    SourceConfigProfile, ToolSourceConfiguration,
    validators::option_validate_source_name,
};

/// The subsystems a pack may contribute to. Used by a consumer's `provides`
/// gate to narrow what a pack registers.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum PackSubsystem {
    Generators,
    Tools,
    Projections,
}

/// Shared base extras carried by every pack source variant: the optional
/// consumer-side subsystem gate.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq,
)]
#[serde(deny_unknown_fields)]
pub struct PackBase {
    /// Consumer-side subsystem gate. When omitted, the pack contributes every
    /// face its manifest declares. Narrowing only; a consumer can never widen
    /// what a pack offers, and the gate cascades to sub-packs.
    #[serde(default)]
    pub provides: Option<Vec<PackSubsystem>>,
}

/// The required declaration-site `id` carried by a `git`/`local` pack source.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq,
)]
#[serde(deny_unknown_fields)]
pub struct PackId {
    pub id: String,
}

/// The optional declaration-site `id` for a (serde-hidden) registry pack
/// source; defaults to the registry name when absent.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq,
)]
#[serde(deny_unknown_fields)]
pub struct PackRegistryId {
    #[serde(default)]
    pub id: Option<String>,
}

/// Profile for pack sources: a shared `provides` gate plus a required
/// declaration-site `id` on `git`/`local` (optional on the hidden registry).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackProfile;

impl SourceConfigProfile for PackProfile {
    type BaseExtra = PackBase;
    type LocalExtra = PackId;
    type GitExtra = PackId;
    type RegistryExtra = PackRegistryId;

    fn declared_id(source: &SourceConfig<Self>) -> Option<&str> {
        match source {
            SourceConfig::Local(local) => Some(local.extra.id.as_str()),
            SourceConfig::Git(git) => Some(git.extra.id.as_str()),
            SourceConfig::Registry(registry) => registry.extra.id.as_deref(),
        }
    }
}

pub type PackSourceConfiguration = SourceConfig<PackProfile>;

/// A `pack.omni.{yaml,yml,json,toml}` shipped at a pack's source root.
///
/// A pack is the subsystem-source slice of a workspace plus author identity and
/// metadata, and nothing ambient. Any combination of the four source lists may
/// be populated; a both-and pack contributes its own sources and expands its
/// sub-packs.
#[derive(Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct PackManifest {
    /// Author-declared canonical identity: scoped, validated by the shared
    /// source-name rule. The display label and future registry coordinate; not
    /// the qualified-id segment (that is the declaration-site `id`).
    pub name: String,

    /// Metadata only; not a version constraint or resolution key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generators: Vec<GeneratorSourceConfiguration>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSourceConfiguration>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<ProjectionSourceConfiguration>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<PackSourceConfiguration>,
}

/// Private, strict intermediate representation. `deny_unknown_fields` here is
/// the ambient-field security boundary: any attempt to set a workspace-global
/// field (`env`, `capabilities`, `enable_experimental`, `ignore`, `ui`,
/// `projects`) is a parse error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackManifest {
    #[serde(default, deserialize_with = "option_validate_source_name")]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    generators: Vec<GeneratorSourceConfiguration>,
    #[serde(default)]
    tools: Vec<ToolSourceConfiguration>,
    #[serde(default)]
    projections: Vec<ProjectionSourceConfiguration>,
    #[serde(default)]
    packs: Vec<PackSourceConfiguration>,
}

impl TryFrom<RawPackManifest> for PackManifest {
    type Error = &'static str;

    fn try_from(raw: RawPackManifest) -> Result<Self, Self::Error> {
        let name = raw.name.ok_or("a pack manifest must declare a `name`")?;

        if raw.generators.is_empty()
            && raw.tools.is_empty()
            && raw.projections.is_empty()
            && raw.packs.is_empty()
        {
            return Err(
                "a pack manifest must declare at least one of `generators`, `tools`, `projections`, or `packs`",
            );
        }

        Ok(Self {
            name,
            version: raw.version,
            description: raw.description,
            generators: raw.generators,
            tools: raw.tools,
            projections: raw.projections,
            packs: raw.packs,
        })
    }
}

impl<'de> Deserialize<'de> for PackManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPackManifest::deserialize(deserializer)?;
        PackManifest::try_from(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<PackManifest, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn minimal_manifest_round_trips() {
        let manifest = parse(
            r#"{"name":"@org/agent-kit","generators":[{"source":"local","path":"./gen"}]}"#,
        )
        .expect("valid manifest");
        assert_eq!(manifest.name, "@org/agent-kit");
        assert_eq!(manifest.generators.len(), 1);

        let re = serde_json::to_string(&manifest).unwrap();
        let back = parse(&re).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn both_and_manifest_with_metadata_round_trips() {
        let manifest = parse(
            r#"{"name":"kit","version":"1.0.0","description":"d","tools":[{"source":"local","path":"./tools"}],"packs":[{"source":"local","path":"./sub","id":"sub"}]}"#,
        )
        .expect("valid manifest");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.packs.len(), 1);

        let re = serde_json::to_string(&manifest).unwrap();
        let back = parse(&re).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn missing_name_is_rejected() {
        assert!(
            parse(r#"{"generators":[{"source":"local","path":"./gen"}]}"#)
                .is_err(),
            "a manifest without a name must be rejected"
        );
    }

    #[test]
    fn all_empty_lists_is_rejected() {
        assert!(
            parse(r#"{"name":"kit"}"#).is_err(),
            "a manifest with no sources must be rejected"
        );
    }

    #[test]
    fn invalid_name_is_rejected() {
        assert!(
            parse(
                r#"{"name":"not a name","generators":[{"source":"local","path":"./gen"}]}"#,
            )
            .is_err(),
            "a name violating the source-name rule must be rejected"
        );
    }

    #[test]
    fn ambient_fields_are_rejected() {
        for field in [
            "env",
            "capabilities",
            "enable_experimental",
            "ignore",
            "ui",
            "projects",
        ] {
            let json = format!(
                r#"{{"name":"kit","generators":[{{"source":"local","path":"./gen"}}],"{field}":{{}}}}"#
            );
            assert!(
                parse(&json).is_err(),
                "ambient field `{field}` must be rejected"
            );
        }
    }

    fn parse_workspace(
        json: &str,
    ) -> Result<crate::WorkspaceConfiguration, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn workspace_with_packs_parses() {
        let ws = parse_workspace(
            r#"{"projects":[],"packs":[{"source":"git","uri":"https://example.com/a.git","rev":"v1","id":"agent-kit"},{"source":"local","path":"./p","id":"house-style"}]}"#,
        )
        .expect("workspace with packs parses");
        assert_eq!(ws.packs.len(), 2);
    }

    #[test]
    fn workspace_rejects_duplicate_pack_id() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"packs":[{"source":"local","path":"./a","id":"dup"},{"source":"local","path":"./b","id":"dup"}]}"#,
            )
            .is_err(),
            "duplicate pack id must be rejected"
        );
    }

    #[test]
    fn workspace_rejects_duplicate_pack_git_uri() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"packs":[{"source":"git","uri":"https://example.com/a.git","rev":"v1","id":"a"},{"source":"git","uri":"https://example.com/a.git","rev":"v2","id":"b"}]}"#,
            )
            .is_err(),
            "duplicate pack git uri must be rejected"
        );
    }

    #[test]
    fn workspace_without_packs_defaults_to_empty() {
        let ws = parse_workspace(r#"{"projects":[]}"#)
            .expect("workspace without packs parses");
        assert!(ws.packs.is_empty());
    }
}
