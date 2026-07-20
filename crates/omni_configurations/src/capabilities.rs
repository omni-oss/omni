//! Workspace-level capabilities: a single, subsystem-tagged capability list.
//!
//! Rather than duplicating a rule once per subsystem, the workspace declares
//! **one** flat [`CapabilityRules<Workspace>`](omni_capabilities::CapabilityRules)
//! and tags each entry with the subsystem(s) it governs via
//! [`WorkspaceScope::subsystem`]. Each subsystem then folds the list into its
//! own typed cascade with `CapabilityRules::reinterpret`, keeping the entries
//! whose tag includes it and giving them that subsystem's default selector.
//!
//! Workspace capabilities are never evaluated directly — they are always
//! reinterpreted into a concrete subsystem profile first, so the [`Workspace`]
//! profile carries only the subsystem selector and no evaluation behaviour.

use omni_capabilities::{CapabilityProfile, NoExtra};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A subsystem whose scripts a workspace capability rule can govern.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Subsystem {
    /// The generator subsystem.
    Generator,
    /// The tools subsystem (declared for tagging; not yet implemented).
    Tools,
}

/// The scalar form of a [`SubsystemSelector`]: the `all` wildcard or a single
/// subsystem.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SubsystemScalar {
    /// Every subsystem — those present today **and any added in the future**.
    /// A live wildcard, not a snapshot of the current enumeration, so a new
    /// subsystem automatically inherits `all`-tagged workspace rules.
    All,
    Generator,
    Tools,
}

/// Which subsystems a workspace capability rule applies to.
///
/// Accepts `"all"`, a single subsystem (`"tools"`), or a list
/// (`["generator", "tools"]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SubsystemSelector {
    /// `"all"` | `"generator"` | `"tools"`.
    Scalar(SubsystemScalar),
    /// `["generator", "tools"]`.
    List(Vec<Subsystem>),
}

impl Default for SubsystemSelector {
    /// An untagged rule governs every subsystem (a workspace-wide floor).
    fn default() -> Self {
        SubsystemSelector::Scalar(SubsystemScalar::All)
    }
}

impl SubsystemSelector {
    /// Whether this selector applies to `subsystem`. `all` always matches,
    /// including subsystems introduced after this rule was written.
    pub fn includes(&self, subsystem: Subsystem) -> bool {
        match self {
            SubsystemSelector::Scalar(SubsystemScalar::All) => true,
            SubsystemSelector::Scalar(SubsystemScalar::Generator) => {
                subsystem == Subsystem::Generator
            }
            SubsystemSelector::Scalar(SubsystemScalar::Tools) => {
                subsystem == Subsystem::Tools
            }
            SubsystemSelector::List(list) => list.contains(&subsystem),
        }
    }
}

/// `applies_to` selector for the [`Workspace`] capability profile.
#[derive(
    Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema,
)]
pub struct WorkspaceScope {
    /// Which subsystems this entry governs. Absent = `all`.
    #[serde(default)]
    pub subsystem: SubsystemSelector,
}

/// The workspace capability profile.
///
/// Its `applies_to` is a [`WorkspaceScope`] (the subsystem tag); it has no
/// per-entry extras and an empty evaluation context, because workspace
/// capabilities are reinterpreted into a subsystem profile before they are ever
/// evaluated. `SUPPORTED` is every domain — whether a specific subsystem can
/// express a given domain is enforced when the list is reinterpreted and
/// validated for that subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Workspace;

impl CapabilityProfile for Workspace {
    const NAME: &'static str = "workspace";

    type AppliesTo = WorkspaceScope;
    type Extra = NoExtra;
    type Context = ();
}

#[cfg(test)]
mod tests {
    use omni_capabilities::CapabilityRules;

    use super::*;

    #[test]
    fn all_matches_every_subsystem() {
        let sel = SubsystemSelector::default();
        assert!(sel.includes(Subsystem::Generator));
        assert!(sel.includes(Subsystem::Tools));
    }

    #[test]
    fn scalar_and_list_forms_deserialize() {
        let scalar: SubsystemSelector =
            serde_json::from_str(r#""tools""#).unwrap();
        assert!(scalar.includes(Subsystem::Tools));
        assert!(!scalar.includes(Subsystem::Generator));

        let all: SubsystemSelector = serde_json::from_str(r#""all""#).unwrap();
        assert!(all.includes(Subsystem::Generator));

        let list: SubsystemSelector =
            serde_json::from_str(r#"["generator"]"#).unwrap();
        assert!(list.includes(Subsystem::Generator));
        assert!(!list.includes(Subsystem::Tools));
    }

    #[test]
    fn entry_without_tag_defaults_to_all() {
        let cfg: CapabilityRules<Workspace> = serde_json::from_str(
            r#"[{ "access": "deny", "domain": "fs.write", "patterns": ["**/.git/**"] }]"#,
        )
        .expect("valid workspace capabilities");
        assert!(
            cfg.as_slice()[0]
                .applies_to
                .subsystem
                .includes(Subsystem::Generator)
        );
        assert!(
            cfg.as_slice()[0]
                .applies_to
                .subsystem
                .includes(Subsystem::Tools)
        );
    }

    #[test]
    fn reinterpret_for_a_subsystem_drops_other_subsystems_rules() {
        // The exact fold a subsystem performs: keep entries whose tag includes
        // it (or `all`), drop the rest. A `tools`-only rule must not leak into
        // the generator floor.
        let ws: CapabilityRules<Workspace> = serde_json::from_str(
            r#"[
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"], "applies_to": { "subsystem": "all" } },
                { "access": "allow", "domain": "net",      "patterns": ["registry:443"], "applies_to": { "subsystem": ["tools"] } }
            ]"#,
        )
        .expect("valid workspace capabilities");

        // Reinterpret into the base profile, keeping generator-governed entries.
        let generator_floor: CapabilityRules = ws.reinterpret(|scope| {
            scope
                .subsystem
                .includes(Subsystem::Generator)
                .then_some(NoExtra {})
        });

        assert_eq!(generator_floor.len(), 1, "tools-only rule must be dropped");
        assert_eq!(
            generator_floor.as_slice()[0].rule.domain,
            omni_capabilities::CapabilityDomain::FsWrite
        );
    }
}
