//! # `omni_capability_enforcement`
//!
//! Turns a **policy** ([`omni_capabilities::RequiredCapabilities`]) into
//! **enforcement** — the concrete, platform- and runtime-specific restrictions
//! that actually confine a script.
//!
//! [`omni_capabilities`] answers *"is this operation allowed?"* in a pure,
//! portable way. This crate answers the orthogonal, messier question: *"can
//! this platform actually enforce that policy, and how?"* — and, crucially,
//! refuses to run when it cannot.
//!
//! ## Layers (defense in depth)
//!
//! An [`EnforcementBackend`] belongs to a [`Tier`]:
//!
//! * [`Tier::PreSpawnFlags`] — restrictions handed to the runtime at launch.
//!   [`DenoFlags`] is the pilot: it lowers the policy into Deno `--allow-*` /
//!   `--deny-*` flags, replacing the blanket `--allow-all` `js_runtime` uses
//!   today. [`NodePermissions`] targets Node's coarser permission model and
//!   shows how a weaker backend's gaps surface as coverage/representability
//!   errors rather than silent holes.
//! * [`Tier::OsSandbox`] — the kernel access-control sandbox for the target OS
//!   ([`NativeOsSandbox`]: Landlock / Seatbelt / AppContainer). Declared per
//!   platform; the integrations themselves are deferred.
//! * [`Tier::InProcessBroker`] — a per-operation broker at omni's I/O boundary
//!   (the generator's `TransactionSys` / the bridge services). [`BridgeBroker`]
//!   is the descriptor: it enforces every mediated domain exactly at runtime,
//!   resolving the representability gaps coarser backends report.
//!
//! Backends compose: [`build_plan`] folds a stack of them into one
//! [`EnforcementPlan`].
//!
//! ## Fail closed
//!
//! Two independent guards, neither of which prompts the user:
//!
//! 1. **Coverage** ([`require_full_coverage`]): every domain the policy locks
//!    down must be enforceable by *some* selected backend, or the run is
//!    refused with the offending domain named. This is the answer to *"what if
//!    the OS can't enforce this?"*.
//! 2. **Representability**: within a covered domain, a specific pattern a
//!    backend cannot express is reported as a [`Gap`] rather than approximated.
//!    Gaps are resolved best-effort against the rest of the stack (a broker
//!    that [enforces the domain exactly](EnforcementBackend::enforces_exactly)
//!    covers them); a *genuinely* unenforceable pattern is then handled per its
//!    effective [`UnenforceablePolicy`], which **defaults to `deny`** and can be
//!    overridden per rule via
//!    [`on_unenforceable`](omni_capabilities::CapabilityRule::on_unenforceable).

#[cfg(target_os = "windows")]
pub mod appcontainer_sandbox;
pub mod backend;
pub mod broker;
pub mod deno;
pub mod error;
#[cfg(target_os = "linux")]
pub mod landlock_sandbox;
mod lower;
pub mod node;
pub mod null;
pub mod plan;
pub mod platform;
#[cfg(target_os = "macos")]
pub mod seatbelt_sandbox;
pub mod shim;
pub mod spawn;

// @anchor:mods

pub use backend::{
    BackendPlan, Coverage, EnforcementBackend, Gap, PatternResolver, Tier,
};
pub use broker::BridgeBroker;
pub use deno::DenoFlags;
pub use error::{EnforcementError, EnforcementErrorKind};
pub use node::NodePermissions;
pub use null::NoSandbox;
pub use omni_capabilities::UnenforceablePolicy;
pub use plan::{
    EnforcementPlan, FloorGap, FloorStrictness, build_plan, build_plan_layered,
    build_plan_strict, build_plan_with, require_full_coverage,
};
pub use platform::{NativeOsSandbox, install_os_sandbox};
pub use shim::{
    SHIM_DOMAINS, ScriptShimBroker, ShimDomain, ShimLayer, ShimPolicy,
};
pub use spawn::{OsSandboxSpec, SpawnPolicy};

// @anchor:uses

#[cfg(test)]
mod tests {
    use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

    use super::*;

    /// End-to-end: a realistic generator-style policy, enforced by a
    /// Deno + OS-sandbox stack, yields the expected launch flags.
    #[test]
    fn end_to_end_generator_policy_to_deno_flags() {
        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@project/generated/**"] },
                { "access": "allow", "domain": "process",  "patterns": ["git"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());

        let roots = PathRoots::new()
            .with(Root::Workspace, "/repo")
            .with(Root::Project, "/repo/pkg");

        // DenoFlags covers everything; NativeOsSandbox is along for the ride
        // (currently no coverage) — the stack is still fully covered.
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &NativeOsSandbox];
        let plan = build_plan(&req, &roots, &backends).expect("fully covered");

        assert!(plan.spawn.args.contains(&"--allow-read=/repo".to_string()));
        assert!(
            plan.spawn
                .args
                .contains(&"--allow-write=/repo/pkg/generated".to_string())
        );
        assert!(plan.spawn.args.contains(&"--allow-run=git".to_string()));
        assert!(!plan.spawn.args.iter().any(|a| a == "--allow-all"));
    }

    /// End-to-end fail-closed: on a hypothetical target where only the (unimpl)
    /// OS sandbox is offered, a policy cannot be enforced and the run is refused.
    #[test]
    fn end_to_end_fails_closed_without_a_capable_backend() {
        let req = project(&CapabilityRules::<()>::new(), &());
        let roots = PathRoots::new().with(Root::Workspace, "/repo");
        let backends: [&dyn EnforcementBackend; 2] =
            [&NoSandbox, &NativeOsSandbox];
        assert!(build_plan(&req, &roots, &backends).is_err());
    }
}
