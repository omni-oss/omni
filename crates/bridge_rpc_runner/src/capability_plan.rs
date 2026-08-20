//! Generic capability *planning* for JS bridge subsystems.
//!
//! Every subsystem that drives scripts through the bridge (generators, tools)
//! confines those scripts under the *same* enforcement model: a cascaded
//! capability policy is projected onto pre-spawn launch flags, an OS sandbox,
//! an in-process broker, and an in-runtime shim. Only the *profile* differs
//! (which domains are expressible, how entries are scoped, what the built-in
//! floor is) — the planning itself is identical.
//!
//! This module owns that shared planning, generic over the capability profile
//! `P` (via [`CapabilityFloors`]). A subsystem keeps only a thin wrapper that
//! fixes `P` to its marker and maps the planning error into its own error type.
//!
//! ## Enforcement is always on when the capabilities feature is enabled
//!
//! A run that declares capabilities is confined to exactly that cascaded
//! policy. A run that declares *none* still runs under the profile's built-in
//! [`CapabilityFloors::default_floor`]. The unconfined fallback
//! ([`build_unconfined_plan`]) is used only when the workspace has not opted
//! into the experimental capabilities feature ([`EffectivePolicy::enforce`] is
//! `false`).

use merge::Merge as _;
use omni_capabilities::{
    CapabilitiesStrictness, CapabilityDomain, CapabilityFloors,
    CapabilityRules, PathRoots, RequiredCapabilities, Root, project,
};
use omni_capability_enforcement::{
    BridgeBroker, DenoFlags, EnforcementBackend, FloorStrictness,
    NativeOsSandbox, NodePermissions, ScriptShimBroker, ShimPolicy,
    SpawnPolicy, UnenforceablePolicy, build_plan_layered,
};
use omni_messages::{
    DiagnosticEvent, diagnostic_event, publish::DiagnosticLevel,
};

use crate::DelegatingJsRuntimeOption;

/// Default cap on nested subsystem invocation depth (a generator nesting
/// `run-generator`, or a pipeline tool invoking another tool). Static cycle
/// detection is the primary guard; this bound is a defense-in-depth backstop
/// for runtime edges a static graph cannot model. Real nesting is only a few
/// levels deep, so the default is generous; callers may raise it when a config
/// legitimately nests deeper.
pub const DEFAULT_MAX_DEPTH: usize = 64;

/// The error surfaced when a capability policy cannot be enforced on the
/// resolved runtime (e.g. a governed domain has no floor under `require-floor`).
/// Subsystems wrap this in their own error type with subsystem-specific wording.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PlanError(pub String);

/// The capability inputs that determine how a bridge script process is launched
/// and confined: the ordered policy `levels` (outermost → innermost: workspace
/// floor, any ancestors, this unit, this action), the `roots` used to resolve
/// `@workspace/…`-style patterns, and the evaluation `context`.
///
/// Levels are kept **distinct** rather than pre-merged so authorization can
/// apply the shrink-only (attenuation) model: each level may only narrow the
/// authority it inherited.
pub struct EffectivePolicy<P: CapabilityFloors> {
    pub levels: Vec<CapabilityRules<P>>,
    pub roots: PathRoots<Root>,
    pub context: P::Context,
    /// How to treat floor gaps for this run (from its configuration).
    pub strictness: CapabilitiesStrictness,
    /// Whether to actually enforce this policy. When `false` (the workspace has
    /// not opted into the experimental capabilities feature) the script runs
    /// unconfined: no OS sandbox, no restrictive launch flags, and a
    /// pass-through broker. The levels/roots/context are still carried so the
    /// policy retains its provenance and can be enforced simply by flipping this.
    pub enforce: bool,
}

// Manual `Clone`/`Debug`: the derived impls would require `P: Clone`/`Debug`
// (never satisfied by a zero-sized marker's associated `Context`), so the bound
// is placed on the associated type instead.
impl<P: CapabilityFloors> Clone for EffectivePolicy<P>
where
    P::Context: Clone,
{
    fn clone(&self) -> Self {
        Self {
            levels: self.levels.clone(),
            roots: self.roots.clone(),
            context: self.context.clone(),
            strictness: self.strictness,
            enforce: self.enforce,
        }
    }
}

impl<P: CapabilityFloors> core::fmt::Debug for EffectivePolicy<P>
where
    P::Context: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EffectivePolicy")
            .field("levels", &self.levels)
            .field("roots", &self.roots)
            .field("context", &self.context)
            .field("strictness", &self.strictness)
            .field("enforce", &self.enforce)
            .finish()
    }
}

impl<P: CapabilityFloors> EffectivePolicy<P>
where
    P::Context: core::fmt::Debug,
{
    /// The policy levels actually enforced, outermost → innermost. Empty levels
    /// (a level that declares nothing) are pure pass-through and dropped. When
    /// *nothing* is declared anywhere, the profile's built-in
    /// [`CapabilityFloors::default_floor`] stands in as the sole level, so
    /// enforcement is always on: an empty declaration means "confined to the
    /// floor", never "unconfined".
    pub fn effective_levels(&self) -> Vec<CapabilityRules<P>> {
        let levels: Vec<CapabilityRules<P>> = self
            .levels
            .iter()
            .filter(|l| !l.is_empty())
            .cloned()
            .collect();
        if levels.is_empty() {
            vec![P::default_floor()]
        } else {
            levels
        }
    }

    /// The effective levels concatenated into a single flat chain: the
    /// conservative **superset** the coarse pre-spawn / OS-sandbox backends
    /// consume. A union of every level's rules can only be wider than the true
    /// per-level intersection, so a launch flag never blocks an operation the
    /// intersection allows. The exact per-operation floor is the layered broker.
    pub fn flat_effective_chain(&self) -> CapabilityRules<P> {
        let mut chain = CapabilityRules::default();
        for level in self.effective_levels() {
            chain.merge(level);
        }
        chain
    }

    /// A stable identity for process caching: processes are shared only among
    /// invocations whose effective policy is identical. The evaluation
    /// `context` and floor `strictness` are part of that identity, **not** just
    /// the levels and roots — `context` selects which `applies_to`-scoped rules
    /// are in force, and `strictness` selects the floor stance the process was
    /// planned under.
    pub fn fingerprint(&self) -> String {
        let levels =
            serde_json::to_string(&self.effective_levels()).unwrap_or_default();
        format!(
            "{levels}|{:?}|{:?}|{:?}|{}",
            self.roots, self.context, self.strictness, self.enforce
        )
    }
}

/// Map a subsystem's configured [`CapabilitiesStrictness`] onto the enforcement
/// layer's [`FloorStrictness`]. `require-floor` promotes every floor gap (a
/// governed domain resting only on a bypassable in-process mechanism) into a
/// hard refusal; `warn` keeps the diagnostic-only behaviour.
pub fn floor_strictness(strictness: CapabilitiesStrictness) -> FloorStrictness {
    match strictness {
        CapabilitiesStrictness::Warn => FloorStrictness::Warn,
        CapabilitiesStrictness::RequireFloor => FloorStrictness::RequireFloor,
    }
}

/// Externally-resolved launch posture threaded into [`build_spawn_plan`].
///
/// The escape-hatch read is done once at the call site
/// ([`from_env`](Self::from_env)) rather than reading the process environment
/// mid-plan, so the plan the runner applies and the posture it claims cannot
/// diverge — and so the disabled posture is directly injectable in tests
/// without mutating process-global state.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnPosture {
    /// Whether the `OMNI_DISABLE_OS_SANDBOX` escape hatch is active for this
    /// run: the OS-sandbox floor is dropped and filesystem confinement falls
    /// back to the bypassable in-process broker.
    pub os_sandbox_disabled: bool,
}

impl SpawnPosture {
    /// Resolve the posture from the process environment (the production path).
    pub fn from_env() -> Self {
        Self {
            os_sandbox_disabled: std::env::var_os("OMNI_DISABLE_OS_SANDBOX")
                .is_some(),
        }
    }
}

/// Plans the fail-closed pre-spawn [`SpawnPolicy`] for an enforced run, together
/// with the [`ShimPolicy`] residual and any **diagnostics** to surface.
///
/// The runtime backend (`deno`/`node`) is composed with the [`NativeOsSandbox`]
/// (Landlock on Linux) and the [`BridgeBroker`] descriptor so that patterns a
/// coarse pre-spawn flag cannot express are resolved by the in-process broker
/// rather than widening access, while the OS sandbox additionally confines the
/// child's *direct* filesystem access at the kernel. The profile's
/// [`CapabilityFloors::baseline_read_chain`] is prepended so the runtime can
/// load the vendored bundle and its own scripts; precise reads are still
/// brokered per operation.
pub fn build_spawn_plan<P: CapabilityFloors>(
    runtime: DelegatingJsRuntimeOption,
    policy: &EffectivePolicy<P>,
    posture: SpawnPosture,
) -> Result<(SpawnPolicy, ShimPolicy, Vec<DiagnosticEvent>), PlanError>
where
    P::Context: core::fmt::Debug,
{
    let mut chain: CapabilityRules<P> = P::baseline_read_chain();
    chain.merge(policy.flat_effective_chain());

    let required = project(&chain, &policy.context);

    // The per-level projections drive the *layered* shim residual so `net`/
    // `process` are attenuated across levels exactly like the broker's `fs`: a
    // deeper level can only narrow an ancestor's allow-list, never widen it. The
    // merged `required` above stays the conservative superset the coarse
    // pre-spawn flags / OS sandbox consume.
    let level_reqs: Vec<RequiredCapabilities> = policy
        .effective_levels()
        .iter()
        .map(|level| project(level, &policy.context))
        .collect();

    let deno = DenoFlags;
    let node = NodePermissions;
    // Use the OS-sandbox posture resolved ONCE by the caller so the floor the
    // plan *claims* matches the confinement the spawner will actually *apply*:
    // the `OMNI_DISABLE_OS_SANDBOX` escape hatch is read a single time into
    // `posture`, and the runtime is marked unconfined when it cannot be placed
    // under the OS mechanism on this platform. Bun cannot boot inside a Windows
    // AppContainer, so the runner launches it unconfined there; an unconfined
    // runtime must not claim the OS-sandbox fs floor, so the plan surfaces an
    // honest floor gap (a hard refusal under `require-floor`) instead of
    // advertising a floor that is never installed.
    let os_sandbox_disabled = posture.os_sandbox_disabled;
    let runtime_confined = !(cfg!(target_os = "windows")
        && runtime == DelegatingJsRuntimeOption::Bun);
    let os = NativeOsSandbox::resolved(os_sandbox_disabled, runtime_confined);
    // The bridge mediates the filesystem routes and `env`. The enforcing `sys`
    // filters `env` by default (`EnvAccess::Filter`): only policy-allowed
    // variable names reach the script's `proc.env()` snapshot, so claiming the
    // broker's `env` coverage here is honest.
    let broker = BridgeBroker::mediating([
        CapabilityDomain::FsRead,
        CapabilityDomain::FsWrite,
        CapabilityDomain::Env,
    ]);
    // The script-level shim enforces `net`/`process` precisely in-runtime for
    // whatever the launch flags could not confine on their own (Node's coarse
    // gates, Bun's absent permission model), so those domains no longer fail
    // closed. The residual it must enforce comes back on `plan.shim`.
    let shim = ScriptShimBroker::new();
    let backends: Vec<&dyn EnforcementBackend> = match runtime {
        DelegatingJsRuntimeOption::Deno => vec![&deno, &os, &broker, &shim],
        DelegatingJsRuntimeOption::Node => vec![&node, &os, &broker, &shim],
        // Bun has no pre-spawn flags; the OS sandbox confines fs and the shim
        // confines net/process at the script boundary.
        DelegatingJsRuntimeOption::Bun => vec![&os, &broker, &shim],
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("runtime is resolved before planning")
        }
    };

    let plan = build_plan_layered(
        &required,
        &level_reqs,
        &policy.roots,
        &backends,
        UnenforceablePolicy::default(),
        floor_strictness(policy.strictness),
    )
    .map_err(|e| PlanError(e.to_string()))?;

    // Two kinds of diagnostic, both routed through the run's subscriber:
    //
    // * `warnings` — a rule that opted into `on_unenforceable: warn` ran with
    //   strictly less confinement than requested.
    // * `floor_gaps` — a governed domain (net/process on Bun, fs off-Linux, …)
    //   is enforced only by the bypassable in-process broker/shim, with no
    //   un-bypassable runtime-flag or OS-sandbox floor for the resolved runtime.
    let mut diagnostics: Vec<DiagnosticEvent> = Vec::new();

    // Loud, distinct signal for the escape hatch: when `OMNI_DISABLE_OS_SANDBOX`
    // is set the OS-sandbox floor is dropped and filesystem confinement falls
    // back to the bypassable in-process broker. This is a real security
    // downgrade that is easy to enable unknowingly, so surface it prominently
    // and up front — ahead of the per-domain floor-gap warnings.
    if os_sandbox_disabled {
        diagnostics.push(diagnostic_event!(
            DiagnosticLevel::Warn,
            "OS-level sandbox is DISABLED for this run \
             (OMNI_DISABLE_OS_SANDBOX is set): filesystem confinement falls \
             back to the bypassable in-process broker and the kernel backstop \
             against direct syscalls is dropped. This is a security downgrade — \
             unset OMNI_DISABLE_OS_SANDBOX to restore OS-level confinement.",
        ));
    }

    for warning in plan.warnings {
        diagnostics.push(diagnostic_event!(
            DiagnosticLevel::Warn,
            "capability policy not fully enforced: {warning}",
        ));
    }
    for gap in plan.floor_gaps {
        diagnostics.push(diagnostic_event!(
            DiagnosticLevel::Warn,
            "capability enforced without an un-bypassable floor: {}",
            gap.reason
        ));
    }

    Ok((plan.spawn, plan.shim, diagnostics))
}

/// The launch policy for the unconfined import-scan runner: **no** OS sandbox
/// (`os_sandbox` stays `None`). Deno enforces its own permission model, so the
/// scan process is granted read + subprocess + env + sys access to drive
/// `deno info` and read the resolved graph; Node and Bun have no pre-spawn
/// permission model and need no flags.
pub fn build_scan_plan(runtime: DelegatingJsRuntimeOption) -> SpawnPolicy {
    let mut spawn = SpawnPolicy::new();
    if runtime == DelegatingJsRuntimeOption::Deno {
        spawn.push_arg("--allow-read");
        spawn.push_arg("--allow-run");
        spawn.push_arg("--allow-env");
        spawn.push_arg("--allow-sys");
    }
    spawn
}

/// The launch policy for an **unenforced** script run — used when the workspace
/// has not opted into the experimental capabilities feature
/// ([`EffectivePolicy::enforce`] is `false`). No OS sandbox and no restrictive
/// flags, restoring the historical unconfined passthrough. Deno defaults to
/// fully locked-down, so it must be explicitly opened with `--allow-all`; Node
/// and Bun impose no restrictive default and need no flags. Paired with
/// [`CapabilityFloors::unconfined_authorizer_chain`], every mediated `sys`
/// operation is then allowed, so the broker is a pure pass-through.
pub fn build_unconfined_plan(
    runtime: DelegatingJsRuntimeOption,
) -> SpawnPolicy {
    let mut spawn = SpawnPolicy::new();
    if runtime == DelegatingJsRuntimeOption::Deno {
        spawn.push_arg("--allow-all");
    }
    spawn
}
