//! Lazily-initialized JavaScript generator script runner(s).
//!
//! A generator run may execute any number of `run-javascript` actions, possibly
//! nested through `run-generator` actions. Each `run-javascript` executes under
//! the capability policy of *its* generator, so a JS process is keyed by its
//! **effective policy** (the runtime plus the cascaded capability chain and path
//! roots), not merely by the runtime:
//!
//! * A process is spawned **lazily**, on the first `run-javascript` action that
//!   needs a given (runtime, policy). Subsequent actions with the *same* policy
//!   — typically every script in the same generator — reuse it.
//! * A nested generator with a *different* policy gets its own process, launched
//!   with its own pre-spawn [`SpawnPolicy`] and its own in-process broker, so a
//!   hijacked script is confined to exactly its generator's authority.
//! * Its file-system / process / log services are backed by the same
//!   [`TransactionSys`] overlay used by the rest of the generator (so JS side
//!   effects participate in the transaction and honour dry runs). That overlay
//!   is always wrapped in a [`PolicyEnforcingSys`] that authorizes every
//!   mediated fs operation against the generator's effective policy before it
//!   runs.
//!
//! ## Enforcement is always on
//!
//! There is no unconfined passthrough. A generator that declares capabilities
//! runs under exactly that cascaded policy. A generator that declares *none*
//! still runs under a built-in **confined floor** ([`default_floor`]): it may
//! read and write within its workspace, but network access, process spawning,
//! and filesystem access outside the workspace are denied. In every case the
//! pre-spawn flags are planned fail-closed and every RPC-mediated fs access is
//! brokered.

use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};

use bridge_rpc_router::Router;
use bridge_rpc_runner::{
    BridgeRunnerOptions, BridgeServiceRunner, DelegatingJsRuntimeOption,
    RunnerPool, VendoredBridgeService,
};
use bridge_rpc_services::{
    RegisterServicesOptions, register_services_with_defaults,
};
use merge::Merge as _;
use omni_capabilities::{
    CapabilityDomain, CapabilityRules, PathRoots, RequiredCapabilities, Root,
    project,
};
use omni_capability_enforcement::{
    BridgeBroker, DenoFlags, EnforcementBackend, FloorStrictness,
    NativeOsSandbox, NodePermissions, ScriptShimBroker, ShimPolicy,
    SpawnPolicy, UnenforceablePolicy, build_plan_layered,
};
use omni_capability_sys::{EvaluatingAuthorizer, PolicyEnforcingSys};
use omni_generator_configurations::{
    CapabilitiesStrictness, Generator, GeneratorContext,
};
use omni_messages::publish::DiagnosticLevel;
use serde::Serialize;

use async_trait::async_trait;

use crate::{GeneratorSys, TransactionSys, error::Error};

/// Path of the `exec-generator-script` service exposed by the bridge service.
const EXEC_GENERATOR_SCRIPT_PATH: &str = "/exec-generator-script";

/// The standard authorizer used to broker a generator's fs operations.
type GeneratorAuthorizer = EvaluatingAuthorizer<Generator, Root>;

/// The capability inputs that determine how a `run-javascript` process is
/// launched and confined: the ordered policy `levels` (outermost → innermost:
/// workspace floor, any ancestor generators, this generator, this action), the
/// `roots` used to resolve `@workspace/…`-style patterns, and the evaluation
/// `context` (the current action / target).
///
/// Levels are kept **distinct** rather than pre-merged so authorization can
/// apply the shrink-only (attenuation) model: each level may only narrow the
/// authority it inherited, so a deeper generator can never grant itself access
/// an ancestor did not (see [`EvaluatingAuthorizer::layered`]).
#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    pub levels: Vec<CapabilityRules<Generator>>,
    pub roots: PathRoots<Root>,
    pub context: GeneratorContext,
    /// How to treat floor gaps for this generator (from its configuration).
    pub strictness: CapabilitiesStrictness,
}

impl EffectivePolicy {
    /// The policy levels actually enforced, outermost → innermost. Empty levels
    /// (a level that declares nothing) are pure pass-through and dropped. When
    /// *nothing* is declared anywhere, the built-in [`default_floor`] stands in
    /// as the sole level, so enforcement is always on: an empty declaration
    /// means "confined to the workspace", never "unconfined".
    fn effective_levels(&self) -> Vec<CapabilityRules<Generator>> {
        let levels: Vec<CapabilityRules<Generator>> = self
            .levels
            .iter()
            .filter(|l| !l.is_empty())
            .cloned()
            .collect();
        if levels.is_empty() {
            vec![default_floor()]
        } else {
            levels
        }
    }

    /// The effective levels concatenated into a single flat chain. This is the
    /// conservative **superset** the coarse pre-spawn / OS-sandbox backends
    /// consume via [`project`]: a union of every level's rules can only be wider
    /// than the true per-level intersection, so a launch flag never blocks an
    /// operation the intersection allows. The exact per-operation floor is the
    /// layered broker ([`EvaluatingAuthorizer::layered`]).
    fn flat_effective_chain(&self) -> CapabilityRules<Generator> {
        let mut chain = CapabilityRules::default();
        for level in self.effective_levels() {
            chain.merge(level);
        }
        chain
    }

    /// A stable identity for process caching: processes are shared only among
    /// `run-javascript` actions whose effective policy is identical.
    fn fingerprint(&self) -> String {
        let levels =
            serde_json::to_string(&self.effective_levels()).unwrap_or_default();
        format!("{levels}|{:?}", self.roots)
    }
}

/// The built-in **confined floor** applied to a generator that declares no
/// capabilities of its own: it may read and write anywhere within its
/// workspace, but everything not granted here — network access, spawning child
/// processes, and filesystem access outside the workspace — is denied
/// (fail-closed). This keeps capability-free generators working (they scaffold
/// files within the workspace) while removing the old unconfined `--allow-all`
/// passthrough.
fn default_floor() -> CapabilityRules<Generator> {
    serde_json::from_str(
        r#"[
            { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
            { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }
        ]"#,
    )
    .expect("built-in floor chain is valid")
}

/// Map the generator's configured [`CapabilitiesStrictness`] onto the
/// enforcement layer's [`FloorStrictness`]. `require-floor` promotes every
/// floor gap (a governed domain resting only on a bypassable in-process
/// mechanism) into a hard refusal; `warn` keeps the shipped diagnostic-only
/// behaviour.
fn floor_strictness(strictness: CapabilitiesStrictness) -> FloorStrictness {
    match strictness {
        CapabilitiesStrictness::Warn => FloorStrictness::Warn,
        CapabilitiesStrictness::RequireFloor => FloorStrictness::RequireFloor,
    }
}

/// Plans the fail-closed pre-spawn [`SpawnPolicy`] for an enforced generator,
/// together with the [`ShimPolicy`] residual and any **diagnostics** to surface.
///
/// Two kinds of diagnostic are produced: rules that opted into
/// `on_unenforceable: warn` (which proceed with strictly less confinement than
/// requested — a `deny`-level gap errors instead; an `allow`-level gap is
/// silent), and **floor gaps** — governed domains that on the resolved runtime
/// have no un-bypassable runtime-flag or OS-sandbox floor and so rest on the
/// bypassable in-process broker/shim alone. Diagnostics are returned rather
/// than logged here so the caller can route them through the run's diagnostic
/// subscriber.
///
/// The runtime backend (`deno`/`node`) is composed with the [`NativeOsSandbox`]
/// (Landlock on Linux) and the [`BridgeBroker`] descriptor so that patterns a
/// coarse pre-spawn flag cannot express (e.g. `deny **/.git/**`) are resolved by
/// the in-process broker rather than widening access, while the OS sandbox
/// additionally confines the child's *direct* filesystem access at the kernel —
/// closing the hole where a script bypasses the bridge to touch the disk itself.
/// A baseline `fs.read @workspace/**` is prepended so the runtime can load the
/// vendored bundle and the generator's own scripts; precise reads are still
/// brokered per operation. Bun has no pre-spawn permission model, so a restricted
/// domain neither the OS sandbox nor the broker can confine (e.g. `process`)
/// makes [`build_plan_strict`] fail closed — the intended outcome.
///
/// ## Require-floor opt-in
///
/// By default, a governed domain that ends up resting only on the bypassable
/// in-process broker/shim (no un-bypassable runtime-flag or OS-sandbox floor —
/// e.g. `net`/`process` on Bun, or `fs` off-Linux) is surfaced as a non-fatal
/// diagnostic. A generator that sets `capabilities: { strictness: require-floor }`
/// promotes that stance to [`FloorStrictness::RequireFloor`], turning every such
/// floor gap into a hard refusal.
fn build_spawn_plan(
    runtime: DelegatingJsRuntimeOption,
    policy: &EffectivePolicy,
) -> Result<(SpawnPolicy, ShimPolicy, Vec<RunScriptDiagnostic>), Error> {
    let mut chain: CapabilityRules<Generator> = serde_json::from_str(
        r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
    )
    .expect("baseline read chain is valid");
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
    let os = NativeOsSandbox;
    // The generator bridge mediates the filesystem routes and `env`. `env` is
    // a generator-governed domain (`Generator::SUPPORTED` includes it), and the
    // enforcing `sys` filters it by default (`EnvAccess::Filter`): only
    // policy-allowed variable names reach the script's `proc.env()` snapshot. So
    // claiming the broker's `env` coverage here is honest — the RPC env service
    // only ever exposes the policy-filtered view.
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
    .map_err(|e| {
        Error::custom(format!(
            "cannot enforce the capability policy for this generator: {e}"
        ))
    })?;

    // Two kinds of diagnostic, both routed through the run's subscriber:
    //
    // * `warnings` — a rule that opted into `on_unenforceable: warn` ran with
    //   strictly less confinement than requested.
    // * `floor_gaps` — a governed domain (net/process on Bun, fs off-Linux, …)
    //   is enforced only by the bypassable in-process broker/shim, with no
    //   un-bypassable runtime-flag or OS-sandbox floor for the resolved runtime.
    let mut diagnostics: Vec<RunScriptDiagnostic> = Vec::new();
    for warning in plan.warnings {
        diagnostics.push(RunScriptDiagnostic::warn(format!(
            "capability policy not fully enforced: {warning}"
        )));
    }
    for gap in plan.floor_gaps {
        diagnostics.push(RunScriptDiagnostic::warn(format!(
            "capability enforced without an un-bypassable floor: {}",
            gap.reason
        )));
    }

    Ok((plan.spawn, plan.shim, diagnostics))
}

type RunnerFuture =
    Pin<Box<dyn Future<Output = Result<BridgeServiceRunner, Error>> + Send>>;
/// Spawns a runner for a concrete (already-resolved) runtime, wrapping the
/// system overlay in the policy broker `authorizer` and launching the process
/// under `spawn_policy`. Enforcement is always on, so an authorizer is always
/// supplied.
type RunnerFactory = Box<
    dyn Fn(
            DelegatingJsRuntimeOption,
            GeneratorAuthorizer,
            SpawnPolicy,
            String,
        ) -> RunnerFuture
        + Send
        + Sync,
>;

/// Parameters handed to a single generator script invocation.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptParams {
    /// Whether the current generator run is a dry run.
    pub dry_run: bool,
    /// Arbitrary, already-templated data provided by the action configuration.
    pub data: serde_json::Value,
    pub output_dir: String,
}

/// A single `{ path, params }` entry in the `exec-generator-script` payload.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptInvocation {
    /// Absolute path of the script to execute.
    pub path: String,
    /// Per-script parameters.
    pub params: ScriptParams,
}

/// A single structured diagnostic produced while running scripts, ready to be
/// forwarded to the run's diagnostic subscriber at its own [`DiagnosticLevel`].
#[derive(Debug, Clone)]
pub struct RunScriptDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

impl RunScriptDiagnostic {
    pub fn warn(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warn,
            message: message.into(),
        }
    }
}

/// The outcome of a [`JsScriptRunner::run_scripts`] dispatch, beyond plain
/// success/failure.
///
/// Today it carries the structured [`diagnostics`](Self::diagnostics) that the
/// caller should surface (e.g. capability warnings for rules that opted into
/// `on_unenforceable: warn` and therefore ran with less confinement than
/// requested). It is the stable seam through which future per-run metadata
/// (timings, spawned-process identity, …) can be returned without changing the
/// trait signature.
#[derive(Debug, Clone, Default)]
pub struct RunScriptResult {
    pub diagnostics: Vec<RunScriptDiagnostic>,
}

/// Abstraction over the JavaScript script execution backend.
///
/// `run_scripts` dispatches one or more script invocations to a JS process for
/// the given runtime and effective capability `policy`, spawning (and confining)
/// that process lazily on first use. It returns a [`RunScriptResult`] whose
/// `diagnostics` the caller routes through the run's diagnostic subscriber.
#[async_trait]
pub trait JsScriptRunner: Send + Sync + std::fmt::Debug {
    async fn run_scripts(
        &self,
        runtime: DelegatingJsRuntimeOption,
        policy: &EffectivePolicy,
        invocations: &[ScriptInvocation],
    ) -> Result<RunScriptResult, Error>;
}

/// A shared, lazily-spawned set of generator script runners keyed by
/// `(runtime, effective-policy fingerprint)`.
///
/// The generic pooling/caching is delegated to [`RunnerPool`]; this type owns
/// only the generator-specific *factory* (the enforcement wiring that turns a
/// resolved runtime + authorizer + [`SpawnPolicy`] into a spawned, confined
/// process).
pub struct LazyScriptRunner {
    pool: RunnerPool<(DelegatingJsRuntimeOption, String)>,
    factory: RunnerFactory,
}

impl std::fmt::Debug for LazyScriptRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyScriptRunner").finish_non_exhaustive()
    }
}

impl LazyScriptRunner {
    /// Creates a runner handle whose backing JS process(es) are spawned on first
    /// use.
    ///
    /// * `sys` is the transactional overlay whose file-system / process / log
    ///   operations are exposed to the JS scripts.
    /// * `context_dir` is where the vendored bundle is materialized and the JS
    ///   process is launched (typically the workspace directory).
    /// * `version` is baked into the vendored bundle so the binary always runs
    ///   the bundle it shipped with.
    pub fn new<S>(
        sys: TransactionSys<S>,
        context_dir: PathBuf,
        version: String,
    ) -> Self
    where
        S: GeneratorSys,
    {
        let factory: RunnerFactory =
            Box::new(move |runtime, authorizer, spawn_policy, shim_json| {
                let sys = sys.clone();
                let context_dir = context_dir.clone();
                let version = version.clone();

                Box::pin(async move {
                    let vendored =
                        VendoredBridgeService::new(version, None::<String>)
                            .ensure(&context_dir)
                            .await
                            .map_err(|e| Error::custom(e.to_string()))?;

                    let mut router = Router::new();
                    // Enforced: broker every mediated fs operation against the
                    // generator's effective policy before it touches `sys`, and
                    // filter `env` by that same policy (`EnvAccess::Filter` is
                    // the default), so the script's `proc.env()` snapshot only
                    // ever sees policy-allowed variables.
                    let enforcing = PolicyEnforcingSys::new(sys, authorizer);
                    register_services_with_defaults(
                        &mut router,
                        Arc::new(enforcing),
                        RegisterServicesOptions::default(),
                    );

                    // The bridge-service CLI expects a `run` subcommand after
                    // its entrypoint. When the runtime's launch flags could not
                    // confine `net`/`process` precisely, the residual policy is
                    // handed to the in-runtime shim via `--enforce <json>` so it
                    // can narrow those calls; an empty residual is omitted (the
                    // shim then does nothing).
                    let mut script_args: Vec<&str> = vec!["run"];
                    if !shim_json.is_empty() {
                        script_args.push("--enforce");
                        script_args.push(&shim_json);
                    }

                    BridgeServiceRunner::spawn(
                        router,
                        BridgeRunnerOptions::new(
                            &vendored.entrypoint,
                            runtime,
                            &spawn_policy,
                        )
                        .with_cwd(Some(&context_dir))
                        .with_script_args(&script_args),
                    )
                    .await
                    .map_err(|e| Error::custom(e.to_string()))
                })
            });

        Self {
            pool: RunnerPool::new(),
            factory,
        }
    }

    /// Shuts down every runner that was started. Best-effort.
    pub async fn shutdown(&self) {
        self.pool.shutdown().await;
    }
}

#[async_trait]
impl JsScriptRunner for LazyScriptRunner {
    async fn run_scripts(
        &self,
        runtime: DelegatingJsRuntimeOption,
        policy: &EffectivePolicy,
        invocations: &[ScriptInvocation],
    ) -> Result<RunScriptResult, Error> {
        let resolved = runtime.resolve().ok_or_else(|| {
            Error::custom("no JS runtime (node/bun/deno) found on PATH")
        })?;

        // Enforcement is always on: a declared policy is used as-is; a generator
        // that declares none is confined to the built-in floor.
        let (spawn_policy, shim_policy, diagnostics) =
            build_spawn_plan(resolved, policy)?;
        let shim_json = if shim_policy.is_empty() {
            String::new()
        } else {
            shim_policy.to_json()
        };
        // Canonicalize the root bases so the enforcing sys can re-authorize a
        // symlink-resolved *real* path without a root that itself lives under a
        // symlink being misread as an escape (see `PolicyEnforcingSys::guard`).
        // `workspace_dir` is already canonical (canonicalized at context load);
        // this also covers `@project`/output roots that may not be. A base that
        // does not (yet) exist is left as-is.
        let roots = policy
            .roots
            .clone()
            .map_bases(|base| std::fs::canonicalize(&base).unwrap_or(base));
        let authorizer = EvaluatingAuthorizer::layered(
            policy.effective_levels(),
            roots,
            policy.context.clone(),
        );

        let key = (resolved, policy.fingerprint());
        let factory = &self.factory;
        let runner = self
            .pool
            .get_or_try_init(key, move || {
                factory(resolved, authorizer, spawn_policy, shim_json)
            })
            .await?;

        runner
            .call(EXEC_GENERATOR_SCRIPT_PATH, invocations)
            .await
            .map_err(|e| Error::custom(e.to_string()))?;

        Ok(RunScriptResult { diagnostics })
    }
}

#[cfg(test)]
mod tests {
    use omni_generator_configurations::CapabilitiesStrictness;

    use super::*;

    fn net_policy(strictness: CapabilitiesStrictness) -> EffectivePolicy {
        // `net` is governed but has no un-bypassable floor on Bun (no pre-spawn
        // flags; the Landlock port floor does not claim `net` coverage), so it
        // is always a floor gap on Bun regardless of host platform.
        let chain: CapabilityRules<Generator> = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        )
        .expect("valid net chain");
        EffectivePolicy {
            levels: vec![chain],
            roots: PathRoots::new().with(Root::Workspace, "/repo"),
            context: GeneratorContext::default(),
            strictness,
        }
    }

    #[test]
    fn strictness_maps_one_to_one_onto_floor_strictness() {
        assert_eq!(
            floor_strictness(CapabilitiesStrictness::Warn),
            FloorStrictness::Warn
        );
        assert_eq!(
            floor_strictness(CapabilitiesStrictness::RequireFloor),
            FloorStrictness::RequireFloor
        );
    }

    #[test]
    fn warn_stance_plans_and_reports_a_net_floor_gap_on_bun() {
        let policy = net_policy(CapabilitiesStrictness::Warn);
        let (_spawn, _shim, diagnostics) =
            build_spawn_plan(DelegatingJsRuntimeOption::Bun, &policy)
                .expect("warn never refuses on a floor gap");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("floor")
                    && d.message.contains("net")),
            "expected a net floor-gap diagnostic, got: {diagnostics:?}"
        );
    }

    #[test]
    fn require_floor_stance_refuses_when_net_has_no_floor_on_bun() {
        let policy = net_policy(CapabilitiesStrictness::RequireFloor);
        let err = build_spawn_plan(DelegatingJsRuntimeOption::Bun, &policy)
            .expect_err(
                "require-floor must refuse an unfloored governed domain",
            );
        // Surfaced as the generator-level enforcement error.
        assert!(
            format!("{err}").contains("cannot enforce"),
            "unexpected error: {err}"
        );
    }
}
