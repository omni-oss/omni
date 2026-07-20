//! Script-level (in-runtime) enforcement: the [`ShimPolicy`] residual and the
//! [`ScriptShimBroker`] descriptor.
//!
//! Some domains cannot be confined by a launch flag on every runtime. `net` and
//! `process` are the prime examples: Deno can express precise `host:port` /
//! program grants, but Node's `--allow-net` / `--allow-child-process` are
//! all-or-nothing and Bun has no permission model at all. Rather than fail
//! closed whenever the runtime is too coarse, omni ships a **shim** inside the
//! JS process (the bridge service) that patches the global `fetch` / child-spawn
//! APIs and authorizes each call against the policy.
//!
//! The division of labour, decided per runtime by [`build_plan`](crate::build_plan):
//!
//! * If the runtime's launch flags enforce a domain **precisely**, the shim does
//!   nothing for it — the residual for that domain is empty.
//! * If the runtime is **too coarse** (a gap), the launch flags grant the
//!   *least-privilege superset* they can (so allowed calls are not blocked at the
//!   kernel/runtime level) and the precise rules are handed to the shim as the
//!   [`ShimPolicy`] residual, which the shim enforces per call.
//!
//! Because the shim only ever *narrows* a runtime grant it can never widen
//! authority. It is a best-effort precision layer for runtimes that cannot do
//! better on their own; the un-bypassable floor is still the runtime flag / OS
//! sandbox.

use std::collections::{BTreeMap, BTreeSet};

use omni_capabilities::{CapabilityDomain, DomainRules, RequiredCapabilities};
use serde::{Deserialize, Serialize};

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError,
    PatternResolver, Tier,
};

/// The domains a script-level shim participates in. Two kinds live here:
///
/// * `net` / `process` — I/O that happens *inside* the JS runtime (network
///   `fetch`, child-process spawn). No RPC broker sees these, so the shim is the
///   only in-process mechanism that can narrow them when the runtime's launch
///   flags are too coarse (Node's all-or-nothing gates, Bun's absent model).
/// * `env` — although environment reads are *also* brokered over RPC (the host
///   materializes a policy-filtered snapshot; see [`BridgeBroker`](crate::BridgeBroker)),
///   `env` has **no OS-sandbox or launch-flag floor on Node/Bun** (Deno alone
///   gates it via `--allow-env`). Marking it a shim domain makes the layered
///   `env` rules travel on the [`ShimPolicy`] so the JS side can filter the
///   `ctx.sys.proc.env()` view in-process as a defense-in-depth twin of the
///   broker's snapshot filter. It stays a [`FloorGap`](crate::FloorGap) on
///   floorless runtimes because a raw `process.env` read (which the confined
///   runtime shares) cannot be safely intercepted in-process — the un-bypassable
///   confinement for sensitive vars is that they are only ever delivered over
///   the broker-filtered RPC channel, never injected into the child's ambient
///   environment.
pub const SHIM_DOMAINS: &[CapabilityDomain] = &[
    CapabilityDomain::Net,
    CapabilityDomain::Process,
    CapabilityDomain::Env,
];

/// The allow/deny patterns the shim must enforce for one domain, in the policy's
/// own neutral vocabulary (`host:port` for `net`, program name/glob for
/// `process`). Serialized to JSON and handed to the runtime shim.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimDomain {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

impl From<DomainRules> for ShimDomain {
    fn from(rules: DomainRules) -> Self {
        // The shim enforces by pattern; the atom's opaque id is planning-only
        // and never crosses the wire, so project each atom down to its pattern.
        Self {
            allow: rules.allow.into_iter().map(|a| a.pattern).collect(),
            deny: rules.deny.into_iter().map(|a| a.pattern).collect(),
        }
    }
}

/// One policy level's shim-relevant rules, keyed by domain — the shim's analog
/// of a single [`CapabilityRules`](omni_capabilities::CapabilityRules)
/// level. A domain the level does **not** constrain is simply absent from the
/// map (it neither grants nor caps — the shim treats it as pass-through for that
/// level, exactly like the `Permit` outcome in the Rust broker fold).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShimLayer {
    pub domains: BTreeMap<CapabilityDomain, ShimDomain>,
}

impl ShimLayer {
    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }
}

/// The residual policy a script-level shim must enforce in-process, **layered**
/// so the shim can apply the same shrink-only (attenuation) fold the Rust broker
/// uses for `fs`: each level may only narrow the authority it inherited, so a
/// deeper/untrusted generator can never widen `net`/`process` past an ancestor's
/// allow-list.
///
/// * [`enforced`](Self::enforced) names the domains the shim is responsible for
///   (the runtime's launch flags could not confine them precisely). A domain
///   absent here is left to the runtime — the shim is a pure pass-through for it.
///   A domain present here but constrained by **no** layer is deny-all by the
///   fold's fail-closed rule (nothing grants it).
/// * [`layers`](Self::layers) carries each policy level's rules, ordered
///   outermost → innermost (workspace floor, ancestor generators, this
///   generator, this action). A layer omits a domain it does not constrain.
///
/// This is the wire artifact passed from the spawning host to the runtime shim
/// (see the bridge service). It is intentionally data-only and
/// serde-serializable so it can travel as a single JSON command-line argument.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimPolicy {
    /// Domains the shim enforces (patches the runtime APIs for). See the type
    /// docs for the deny-all-when-ungranted rule.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub enforced: BTreeSet<CapabilityDomain>,
    /// Per-level rules, outermost → innermost, folded by attenuation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<ShimLayer>,
}

impl ShimPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the shim enforces nothing (no domain to patch → pure
    /// passthrough, and `--enforce` can be omitted entirely).
    pub fn is_empty(&self) -> bool {
        self.enforced.is_empty()
    }

    /// Serialize to the compact JSON form passed to the runtime shim.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// The script-level shim descriptor.
///
/// Like [`BridgeBroker`](crate::BridgeBroker) it is a pure descriptor: it emits
/// no launch flags and reports no gaps. Its role is twofold:
///
/// 1. It declares [coverage](EnforcementBackend::coverage) for the
///    [`SHIM_DOMAINS`] and [`enforces_exactly`](EnforcementBackend::enforces_exactly),
///    so the coarse representability [`Gap`](crate::Gap)s that pre-spawn flag
///    backends report for `net`/`process` are resolved instead of failing closed.
/// 2. It marks those domains via [`shim_domains`](EnforcementBackend::shim_domains)
///    so [`build_plan`](crate::build_plan) knows to compute the [`ShimPolicy`]
///    residual for them.
///
/// A stack **without** this backend keeps the old fail-closed behaviour: a
/// coarse `net`/`process` pattern remains a genuine gap subject to the
/// configured [`UnenforceablePolicy`](crate::UnenforceablePolicy).
#[derive(Debug, Clone)]
pub struct ScriptShimBroker {
    coverage: Coverage,
}

impl Default for ScriptShimBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptShimBroker {
    /// A shim enforcing the default [`SHIM_DOMAINS`] (`net`, `process`, `env`).
    pub fn new() -> Self {
        Self {
            coverage: Coverage::of(SHIM_DOMAINS.iter().copied()),
        }
    }

    /// A shim enforcing an explicit domain set — for callers whose runtime shim
    /// patches a different set of APIs.
    pub fn mediating(
        domains: impl IntoIterator<Item = CapabilityDomain>,
    ) -> Self {
        Self {
            coverage: Coverage::of(domains),
        }
    }
}

impl EnforcementBackend for ScriptShimBroker {
    fn name(&self) -> &'static str {
        "script-shim"
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

    fn shim_domains(&self) -> Coverage {
        self.coverage.clone()
    }

    fn plan(
        &self,
        _req: &RequiredCapabilities,
        _roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        // A descriptor: the residual it is responsible for is computed centrally
        // by `build_plan` (which alone knows which domains the launch flags
        // already covered precisely).
        Ok(BackendPlan::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_and_marks_the_shim_domains() {
        let shim = ScriptShimBroker::new();
        assert!(shim.enforces_exactly());
        assert!(shim.coverage().covers(CapabilityDomain::Net));
        assert!(shim.coverage().covers(CapabilityDomain::Process));
        assert!(shim.coverage().covers(CapabilityDomain::Env));
        assert!(shim.shim_domains().covers(CapabilityDomain::Net));
        assert!(shim.shim_domains().covers(CapabilityDomain::Env));
        assert!(!shim.coverage().covers(CapabilityDomain::FsRead));
    }

    #[test]
    fn shim_policy_json_roundtrips_and_omits_empty() {
        let mut p = ShimPolicy::new();
        p.enforced.insert(CapabilityDomain::Net);
        let mut layer = ShimLayer::default();
        layer.domains.insert(
            CapabilityDomain::Net,
            ShimDomain {
                allow: vec!["example.com:443".to_string()],
                deny: vec![],
            },
        );
        p.layers.push(layer);

        let json = p.to_json();
        assert!(json.contains("enforced"), "{json}");
        assert!(json.contains("layers"), "{json}");
        assert!(json.contains("net"), "{json}");
        assert!(json.contains("example.com:443"), "{json}");
        // `deny: []` is omitted.
        assert!(!json.contains("deny"), "{json}");
        let back: ShimPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn an_empty_shim_policy_serializes_to_an_empty_object() {
        // No enforced domains and no layers → both fields elided, so an empty
        // residual is `{}` and `is_empty()` holds (the caller omits
        // `--enforce`).
        let p = ShimPolicy::new();
        assert!(p.is_empty());
        assert_eq!(p.to_json(), "{}");
    }

    #[test]
    fn a_domain_enforced_with_no_granting_layer_is_deny_all() {
        // `enforced` marks the domain even when no layer grants it: the shim
        // must patch the API and (by the fold's fail-closed rule) deny every
        // call. `is_empty()` is false so `--enforce` is passed.
        let mut p = ShimPolicy::new();
        p.enforced.insert(CapabilityDomain::Process);
        assert!(!p.is_empty());
        let json = p.to_json();
        assert!(json.contains("process"), "{json}");
        let back: ShimPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
