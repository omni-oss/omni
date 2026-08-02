//! [`BridgeBroker`]: the [`Tier::InProcessBroker`] descriptor for omni's own
//! I/O boundary.
//!
//! Unlike the pre-spawn backends ([`DenoFlags`](crate::DenoFlags),
//! [`NodePermissions`](crate::NodePermissions)), the broker does not translate
//! the policy into launch flags. It represents a *runtime* mechanism: every
//! filesystem/env operation an untrusted script performs is routed through
//! omni's `sys` handle (see `omni_capability_sys::PolicyEnforcingSys`) and
//! authorized against the full policy at the moment it happens. That makes it a
//! **per-operation, exact** enforcer — it can honor any pattern the pure engine
//! can evaluate, including the precise globs (`deny **/.git/**`, mid-path
//! filters) that path-prefix flag backends must report as [`Gap`]s.
//!
//! ## What it covers, and why not everything
//!
//! A broker only enforces what actually travels through omni's boundary. Today
//! the bridge mediates **filesystem routes and the process `ENV`/`SNAPSHOT`
//! services**, so the default coverage is `fs.read` / `fs.write` / `env`. There
//! is no omni-mediated service for opening raw sockets or spawning child
//! processes, so `net` and `process` are deliberately *not* covered: a script
//! that bypasses the bridge to open a socket directly would not be seen. Those
//! domains must be confined by a runtime flag / OS-sandbox backend instead.
//!
//! This is a pure descriptor: it carries no dependency on the bridge crates.
//! Its job is to tell [`build_plan`](crate::build_plan) which domains are
//! resolved exactly at runtime, so the gaps coarser backends report for those
//! domains are not treated as genuine.

use omni_capabilities::{CapabilityDomain, RequiredCapabilities};

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError,
    PatternResolver, Tier,
};

const NAME: &str = "bridge-broker";

/// The set of domains omni's bridge mediates by default: filesystem access
/// (routed through the fs services) and environment reads (the `ENV` /
/// `SNAPSHOT` services). See [`BridgeBroker`].
const DEFAULT_MEDIATED: &[CapabilityDomain] = &[
    CapabilityDomain::FsRead,
    CapabilityDomain::FsWrite,
    CapabilityDomain::Env,
];

/// The in-process broker backend for omni's bridge boundary.
///
/// Because every mediated operation is authorized against the live policy,
/// [`enforces_exactly`](EnforcementBackend::enforces_exactly) is `true`: the
/// broker resolves the representability [`Gap`]s that pre-spawn backends report
/// for the domains it covers.
#[derive(Debug, Clone)]
pub struct BridgeBroker {
    coverage: Coverage,
}

impl Default for BridgeBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeBroker {
    /// A broker mediating the default domains (`fs.read`, `fs.write`, `env`).
    ///
    /// The `env` claim is truthful because the wrapping `PolicyEnforcingSys`
    /// filters the environment by default (`EnvAccess::Filter` — only
    /// policy-allowed variable names reach the script). A caller that
    /// deliberately opts *out* of env filtering (`EnvAccess::Passthrough`) must
    /// construct the broker with [`mediating`](Self::mediating) over `fs` only,
    /// so the coverage claim never outruns the enforcement.
    pub fn new() -> Self {
        Self {
            coverage: Coverage::of(DEFAULT_MEDIATED.iter().copied()),
        }
    }

    /// A broker mediating an explicit set of domains — for callers whose bridge
    /// exposes a different (narrower or wider) set of services.
    ///
    /// Only pass domains the bridge *actually* routes through omni's `sys`
    /// handle; claiming coverage for a domain a script can reach directly would
    /// create a silent hole.
    pub fn mediating(
        domains: impl IntoIterator<Item = CapabilityDomain>,
    ) -> Self {
        Self {
            coverage: Coverage::of(domains),
        }
    }
}

impl EnforcementBackend for BridgeBroker {
    fn name(&self) -> &'static str {
        NAME
    }

    fn tier(&self) -> Tier {
        Tier::InProcessBroker
    }

    fn coverage(&self) -> Coverage {
        self.coverage.clone()
    }

    fn enforces_exactly(&self) -> bool {
        true
    }

    fn plan(
        &self,
        _req: &RequiredCapabilities,
        _roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        // The broker enforces at runtime, not at launch: it emits no spawn
        // flags and — crucially — reports no gaps, because it can honor any
        // pattern the policy engine evaluates.
        Ok(BackendPlan::new())
    }
}

#[cfg(test)]
mod tests {
    use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

    use super::*;
    use crate::{DenoFlags, build_plan};

    fn roots() -> PathRoots {
        PathRoots::new().with(Root::Workspace, "/repo")
    }

    fn require(json: &str) -> RequiredCapabilities {
        let cfg: CapabilityRules = serde_json::from_str(json).unwrap();
        project(&cfg, &())
    }

    #[test]
    fn is_an_exact_runtime_broker() {
        let b = BridgeBroker::new();
        assert_eq!(b.tier(), Tier::InProcessBroker);
        assert!(b.enforces_exactly());
        assert_eq!(b.name(), "bridge-broker");
    }

    #[test]
    fn default_covers_fs_and_env_only() {
        let cov = BridgeBroker::new().coverage();
        assert!(cov.covers(CapabilityDomain::FsRead));
        assert!(cov.covers(CapabilityDomain::FsWrite));
        assert!(cov.covers(CapabilityDomain::Env));
        // No omni-mediated service opens sockets or spawns processes.
        assert!(!cov.covers(CapabilityDomain::Net));
        assert!(!cov.covers(CapabilityDomain::Process));
    }

    #[test]
    fn mediating_overrides_the_default_set() {
        let cov =
            BridgeBroker::mediating([CapabilityDomain::FsRead]).coverage();
        assert!(cov.covers(CapabilityDomain::FsRead));
        assert!(!cov.covers(CapabilityDomain::Env));
    }

    #[test]
    fn contributes_no_spawn_flags_or_gaps() {
        let req = require(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let plan = BridgeBroker::new()
            .plan(&req, &roots())
            .expect("broker plan is infallible");
        assert!(plan.spawn.args.is_empty());
        assert!(plan.gaps.is_empty());
    }

    #[test]
    fn resolves_deno_fs_deny_gaps_for_mediated_domains() {
        // Deno cannot express `deny **/.git/**` as a path prefix and reports a
        // gap; the broker covers fs.write exactly, so the stack has no genuine
        // gap even under the default `deny` policy.
        let req = require(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &BridgeBroker::new()];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("broker resolves the fs.write gap");
        assert!(plan.spawn.args.contains(&"--allow-write=/repo".to_string()));
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn does_not_resolve_gaps_outside_its_coverage() {
        // A wildcard-host net rule is a Deno gap. The broker does NOT cover
        // `net`, so the gap stays genuine and the default policy fails closed.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["*.npmjs.org:443"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &BridgeBroker::new()];
        let err = build_plan(&req, &roots(), &backends)
            .expect_err("net wildcard is not broker-mediated");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::Unenforceable);
    }
}
