//! The **base** capability profile — the pure-data `()` profile.
//!
//! This is the counterpart to [`generator`](crate::generator): where `Generator`
//! specializes the model for the generator subsystem, `()` is the neutral
//! profile used by tooling that reads or manipulates capability configs without
//! belonging to any particular subsystem.
//!
//! It has no `applies_to` selector, no per-entry extras, an empty context, and
//! supports every [`CapabilityDomain`](crate::CapabilityDomain). Every rule
//! therefore applies unconditionally, and decisions use the default
//! fail-closed, deny-dominant strategy.

use crate::{CapabilityProfile, NoExtra};

impl CapabilityProfile for () {
    const NAME: &'static str = "capabilities";

    type AppliesTo = NoExtra;
    type Extra = NoExtra;
    type Context = ();
    // SUPPORTED, applies, decide, merge_entry all use their defaults.
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{CapabilityRules, PathRoots, Request, Root, evaluate};

    fn parse(json: &str) -> CapabilityRules {
        serde_json::from_str(json).expect("valid capabilities config")
    }

    #[test]
    fn base_profile_evaluates_without_a_subsystem() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let roots = PathRoots::<Root>::new();

        let allowed = evaluate(
            &cfg,
            &Request::Net {
                host: "example.com",
                port: 443,
            },
            &roots,
            &(),
        );
        assert!(allowed.is_allowed());

        let denied = evaluate(
            &cfg,
            &Request::Net {
                host: "evil.com",
                port: 443,
            },
            &roots,
            &(),
        );
        assert!(denied.is_denied());
    }

    #[test]
    fn base_profile_is_fail_closed() {
        let cfg = CapabilityRules::<()>::new();
        let d = evaluate(
            &cfg,
            &Request::Fs {
                write: true,
                path: Path::new("/anything"),
            },
            &PathRoots::<Root>::new(),
            &(),
        );
        assert!(d.is_denied());
    }
}
