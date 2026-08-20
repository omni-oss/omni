//! The **tool** capability profile.
//!
//! This wires the tool subsystem into `omni_capabilities` by implementing
//! [`CapabilityProfile`] on the [`Tool`] marker, mirroring the generator
//! profile. Tools have no actions or targets, so the `applies_to` selector and
//! evaluation context carry no extras: the policy cascade is simply
//! workspace -> tool.

use omni_capabilities::{
    CapabilityDomain, CapabilityFloors, CapabilityProfile, NoExtra,
};

// Re-exported so the `omni_tool_configurations::CapabilitiesStrictness` /
// `CapabilityPolicyConfig` paths resolve without depending on
// `omni_capabilities` directly.
pub use omni_capabilities::{CapabilitiesStrictness, CapabilityPolicyConfig};

/// Capability-policy marker for tools.
///
/// A tool declares its policy as `CapabilityRules<Tool>`. Unlike the generator
/// profile, tools have no per-entry selector (no actions/targets) and an empty
/// evaluation context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tool;

impl CapabilityProfile for Tool {
    const SUPPORTED: &'static [CapabilityDomain] = &[
        CapabilityDomain::FsRead,
        CapabilityDomain::FsWrite,
        CapabilityDomain::Process,
        CapabilityDomain::Net,
        CapabilityDomain::Env,
    ];
    const NAME: &'static str = "tool";

    type AppliesTo = NoExtra;
    type Extra = NoExtra;
    type Context = ();
    // Uses the default `applies` (always) and the default fail-closed,
    // deny-dominant `decide`.
}

// Tools use the shared default chains (confined to `@workspace/**`). By design
// tools do not distinguish local from remote (`git`) sources for confinement —
// the workspace floor is the meaningful boundary for both — so this uses the
// default `CapabilityFloors` chains without overriding `default_floor`.
impl CapabilityFloors for Tool {}

#[cfg(test)]
mod tests {
    use omni_capabilities::{
        CapabilityRules, PathRoots, Request, Root, evaluate, validate,
    };

    use super::*;

    fn parse(json: &str) -> CapabilityRules<Tool> {
        serde_json::from_str(json).expect("valid tool capabilities")
    }

    #[test]
    fn all_five_domains_are_supported_by_validate() {
        for domain in ["fs.read", "fs.write", "process", "net", "env"] {
            let cfg = parse(&format!(
                r#"[{{ "access": "allow", "domain": "{domain}", "patterns": ["**"] }}]"#
            ));
            validate(&cfg).unwrap_or_else(|_| {
                panic!("{domain} is a supported tool domain")
            });
        }
    }

    #[test]
    fn deny_dominates_allow() {
        let cfg = parse(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["example.com:443"] },
                { "access": "deny",  "domain": "net", "patterns": ["example.com:443"] }
            ]"#,
        );
        let roots = PathRoots::<Root>::new();
        assert!(
            evaluate(
                &cfg,
                &Request::Net {
                    host: "example.com",
                    port: 443
                },
                &roots,
                &()
            )
            .is_denied()
        );
    }

    #[test]
    fn fail_closed_when_no_rule_matches() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let roots = PathRoots::<Root>::new();
        assert!(
            evaluate(
                &cfg,
                &Request::Net {
                    host: "other.com",
                    port: 443
                },
                &roots,
                &()
            )
            .is_denied()
        );
    }
}
