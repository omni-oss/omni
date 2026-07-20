//! The **generator** capability profile.
//!
//! This wires the generator subsystem into `omni_capabilities` by implementing
//! [`CapabilityProfile`] on the very same [`Generator`] marker that already
//! implements `omni_input_schema::InputProfile`. One marker therefore describes
//! *both* the generator's input schema and its capability policy, mirroring the
//! profile pattern the input schema established.
//!
//! Specialization:
//! * `SUPPORTED` restricts generators to filesystem, process, and network
//!   domains. Network access is confined by the script-level shim (a patched
//!   `fetch`) fed the residual policy, so a generator can fetch remote resources
//!   only within its declared `net` allow-list.
//! * `AppliesTo` = [`GeneratorScope`] scopes an entry to specific actions /
//!   targets.
//! * `Context` = [`GeneratorContext`] carries the action/target currently
//!   executing.

use omni_capabilities::{CapabilityDomain, CapabilityProfile, NoExtra};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionConfigurationType, Generator};

// `CapabilitiesStrictness` now lives in `omni_capabilities` alongside
// `CapabilityPolicyConfig` so every configuration level (workspace, generator,
// action) can share one generic policy shape. Re-exported here to preserve the
// `omni_generator_configurations::CapabilitiesStrictness` path.
pub use omni_capabilities::{CapabilitiesStrictness, CapabilityPolicyConfig};

/// Generator-specific `applies_to` selector.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema,
)]
pub struct GeneratorScope {
    /// Action names this entry is scoped to. Empty = any action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionConfigurationType>,
    /// Target keys this entry is scoped to. Empty = any target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

/// Evaluation context for a generator run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GeneratorContext {
    /// The action currently executing, if any.
    pub action: Option<String>,
    /// The target currently being written, if any.
    pub target: Option<String>,
}

impl CapabilityProfile for Generator {
    const SUPPORTED: &'static [CapabilityDomain] = &[
        CapabilityDomain::FsRead,
        CapabilityDomain::FsWrite,
        CapabilityDomain::Process,
        CapabilityDomain::Net,
        CapabilityDomain::Env,
    ];
    const NAME: &'static str = "generator";

    type AppliesTo = GeneratorScope;
    type Extra = NoExtra;
    type Context = GeneratorContext;

    fn applies(applies_to: &GeneratorScope, ctx: &GeneratorContext) -> bool {
        let action_ok = applies_to.actions.is_empty()
            || ctx.action.as_ref().is_some_and(|a| {
                applies_to.actions.iter().any(|x| x.as_ref() == a)
            });
        let target_ok = applies_to.targets.is_empty()
            || ctx
                .target
                .as_ref()
                .is_some_and(|t| applies_to.targets.iter().any(|x| x == t));
        action_ok && target_ok
    }
    // Uses the default fail-closed, deny-dominant `decide`.
}

#[cfg(test)]
mod tests {
    use omni_capabilities::{
        CapabilityRules, PathRoots, Request, Root, evaluate, validate,
    };

    use super::*;

    fn parse(json: &str) -> CapabilityRules<Generator> {
        serde_json::from_str(json).expect("valid capabilities config")
    }

    #[test]
    fn deserializes_applies_to_actions() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git"],
                  "applies_to": { "actions": ["run-command"] } }]"#,
        );
        assert_eq!(
            cfg.as_slice()[0].applies_to.actions,
            vec![ActionConfigurationType::RunCommand]
        );
    }

    #[test]
    fn action_scoped_rule_only_applies_to_that_action() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git"],
                  "applies_to": { "actions": ["run-command"] } }]"#,
        );
        let roots = PathRoots::<Root>::new();

        let matching = GeneratorContext {
            action: Some("run-command".into()),
            target: None,
        };
        let other = GeneratorContext {
            action: Some("add".into()),
            target: None,
        };

        assert!(
            evaluate(
                &cfg,
                &Request::Process { program: "git" },
                &roots,
                &matching
            )
            .is_allowed()
        );
        assert!(
            evaluate(
                &cfg,
                &Request::Process { program: "git" },
                &roots,
                &other
            )
            .is_denied()
        );
    }

    #[test]
    fn target_scoped_rule_only_applies_to_that_target() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["**"],
                  "applies_to": { "targets": ["src"] } }]"#,
        );
        let roots = PathRoots::<Root>::new();
        let path = std::path::Path::new("/repo/x");

        let src = GeneratorContext {
            action: None,
            target: Some("src".into()),
        };
        let docs = GeneratorContext {
            action: None,
            target: Some("docs".into()),
        };

        assert!(
            evaluate(&cfg, &Request::Fs { write: true, path }, &roots, &src)
                .is_allowed()
        );
        assert!(
            evaluate(&cfg, &Request::Fs { write: true, path }, &roots, &docs)
                .is_denied()
        );
    }

    #[test]
    fn net_domain_is_supported_by_validate() {
        // Generators may declare `net`; it is enforced by the script-level shim.
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        validate(&cfg).expect("net is a supported generator domain");
    }

    #[test]
    fn env_domain_is_supported_by_validate() {
        // Generators still do not govern environment reads.
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["HOME"] }]"#,
        );

        validate(&cfg).expect("env is a supported generator domain");
    }
}
