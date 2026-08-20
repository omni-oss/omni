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

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

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
use omni_messages::{
    DiagnosticEvent, diagnostic_event, publish::DiagnosticLevel,
};
use serde::Serialize;
use system_traits::EnvSnapshot;

use async_trait::async_trait;

use crate::{
    GeneratorSys, TransactionSys,
    error::Error,
    import_scan::{ClosureCache, governing_manifests, scan_closure},
};

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
    /// Whether to actually enforce this policy. When `false` (the workspace has
    /// not opted into the experimental capabilities feature) the script runs
    /// unconfined: no OS sandbox, no restrictive launch flags, and a
    /// pass-through broker. The levels/roots/context are still carried so the
    /// policy retains its provenance and can be enforced simply by flipping this.
    pub enforce: bool,
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
    /// `run-javascript` actions whose effective policy is identical. The
    /// evaluation `context` and floor `strictness` are part of that identity,
    /// **not** just the levels and roots. `context` selects which
    /// `applies_to`-scoped rules are in force, so two actions with identical
    /// levels but different action/target contexts must not share a confined
    /// process — doing so would let one action run under the other's
    /// context-scoped authority (and under the flags/shim planned for it).
    /// `strictness` selects the floor stance the process was planned under.
    fn fingerprint(&self) -> String {
        let levels =
            serde_json::to_string(&self.effective_levels()).unwrap_or_default();
        format!(
            "{levels}|{:?}|{:?}|{:?}|{}",
            self.roots, self.context, self.strictness, self.enforce
        )
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
/// Externally-resolved launch posture threaded into [`build_spawn_plan`].
///
/// The escape-hatch read is done once at the call site
/// ([`from_env`](Self::from_env)) rather than reading the process environment
/// mid-plan, so the plan the runner applies and the posture it claims cannot
/// diverge — and so the disabled posture is directly injectable in tests
/// without mutating process-global state.
#[derive(Debug, Clone, Copy, Default)]
struct SpawnPosture {
    /// Whether the `OMNI_DISABLE_OS_SANDBOX` escape hatch is active for this
    /// run: the OS-sandbox floor is dropped and filesystem confinement falls
    /// back to the bypassable in-process broker.
    os_sandbox_disabled: bool,
}

impl SpawnPosture {
    /// Resolve the posture from the process environment (the production path).
    fn from_env() -> Self {
        Self {
            os_sandbox_disabled: std::env::var_os("OMNI_DISABLE_OS_SANDBOX")
                .is_some(),
        }
    }
}

fn build_spawn_plan(
    runtime: DelegatingJsRuntimeOption,
    policy: &EffectivePolicy,
    posture: SpawnPosture,
) -> Result<(SpawnPolicy, ShimPolicy, Vec<DiagnosticEvent>), Error> {
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
    // Use the OS-sandbox posture resolved ONCE by the caller so the floor the
    // plan *claims* matches the confinement the spawner will actually *apply*
    // (T17/T18): the `OMNI_DISABLE_OS_SANDBOX` escape hatch is read a single
    // time into `posture`, and the runtime is marked unconfined when it cannot
    // be placed under the OS mechanism on this platform. Bun cannot boot inside
    // a Windows AppContainer, so the runner launches it unconfined there (see
    // `bridge_rpc_runner::build_command`); an unconfined runtime must not claim
    // the OS-sandbox fs floor, so the plan surfaces an honest floor gap (a hard
    // refusal under `require-floor`) instead of advertising a floor that is
    // never installed.
    let os_sandbox_disabled = posture.os_sandbox_disabled;
    let runtime_confined = !(cfg!(target_os = "windows")
        && runtime == DelegatingJsRuntimeOption::Bun);
    let os = NativeOsSandbox::resolved(os_sandbox_disabled, runtime_confined);
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
    let mut diagnostics: Vec<DiagnosticEvent> = Vec::new();

    // Loud, distinct signal for the escape hatch: when `OMNI_DISABLE_OS_SANDBOX`
    // is set the OS-sandbox floor is dropped and filesystem confinement falls
    // back to the bypassable in-process broker. This is a real security
    // downgrade that is easy to enable unknowingly (inherited env in CI, a
    // stale shell export), so surface it prominently and up front — ahead of
    // the per-domain floor-gap warnings — rather than letting it hide among the
    // ordinary gap diagnostics it induces.
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

    // Windows note: `bun` cannot boot inside an AppContainer (it `stat`s every
    // CWD ancestor up to `C:\`, which a Low-integrity container is denied), so
    // the runner launches it *unconfined* there. That is already reflected in
    // the plan: `NativeOsSandbox::resolved(.., runtime_confined = false)` above
    // makes the OS tier claim no fs floor for Bun on Windows, so the missing
    // floor is surfaced through the normal `floor_gaps` path (and refused under
    // `require-floor`) rather than needing a bespoke diagnostic here.

    Ok((plan.spawn, plan.shim, diagnostics))
}

/// The permissive capability chain the **unconfined** import-scan runner is
/// authorized under. Resolution is omni's own trusted tooling: it drives the
/// runtime's resolver over the tree and reads the scanned files directly (never
/// executing them), so it is granted every domain. In practice this only widens
/// the env snapshot the scan runtime inherits (so `deno info` and the resolver
/// see the ambient env); the scan performs its filesystem reads through the
/// runtime's own APIs, not the brokered `sys`.
fn scan_authorizer_chain() -> CapabilityRules<Generator> {
    serde_json::from_str(
        r#"[
            { "access": "allow", "domain": "fs.read",  "patterns": ["**"] },
            { "access": "allow", "domain": "fs.write", "patterns": ["**"] },
            { "access": "allow", "domain": "process",  "patterns": ["*"] },
            { "access": "allow", "domain": "env",      "patterns": ["*"] }
        ]"#,
    )
    .expect("scan authorizer chain is valid")
}

/// The launch policy for the unconfined import-scan runner: **no** OS sandbox
/// (`os_sandbox` stays `None`), so on Windows it is an ordinary child with full
/// filesystem read. Deno additionally enforces its own permission model, so the
/// scan process is granted read + subprocess + env + sys access to drive
/// `deno info` and read the resolved graph; Node and Bun have no pre-spawn
/// permission model and need no flags.
fn build_scan_plan(runtime: DelegatingJsRuntimeOption) -> SpawnPolicy {
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
/// ([`EffectivePolicy::enforce`] is `false`). No OS sandbox (`os_sandbox` stays
/// `None`) and no restrictive flags, restoring the historical unconfined
/// passthrough. Deno defaults to fully locked-down, so it must be explicitly
/// opened with `--allow-all`; Node and Bun impose no restrictive default and
/// need no flags. Paired with [`unconfined_authorizer_chain`], every mediated
/// `sys` operation is then allowed, so the broker is a pure pass-through.
fn build_unconfined_plan(runtime: DelegatingJsRuntimeOption) -> SpawnPolicy {
    let mut spawn = SpawnPolicy::new();
    if runtime == DelegatingJsRuntimeOption::Deno {
        spawn.push_arg("--allow-all");
    }
    spawn
}

/// The allow-everything capability chain used to authorize an **unenforced**
/// script run. Every mediated domain (fs read/write, `env`) is granted, so the
/// in-process broker never denies an operation and the child's `env` snapshot
/// is the full ambient environment — matching an unconfined runtime.
fn unconfined_authorizer_chain() -> CapabilityRules<Generator> {
    scan_authorizer_chain()
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
    pub diagnostics: Vec<DiagnosticEvent>,
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

/// Optional per-`call` execution timeout, read from `OMNI_JS_EXEC_TIMEOUT`
/// (whole seconds). Unset (or unparseable / `0`) means no cap — a script is
/// bounded only by the runtime exiting — preserving the historical behaviour for
/// legitimately long-running generators. Operators who want to bound a hung
/// script set the variable; the runner then kills the runtime on expiry.
fn exec_timeout_from_env() -> Option<Duration> {
    std::env::var("OMNI_JS_EXEC_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
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
    /// An **unconfined** runner per runtime used only to compute the import
    /// closure (§5.5). It runs omni's own trusted resolve tooling, so it must
    /// reach `package.json`/`tsconfig`/`node_modules` across the tree — the very
    /// reads that are expensive to grant under a confined child — and hand back
    /// only a bounded path list. Reused across calls for the same runtime.
    scan_pool: RunnerPool<DelegatingJsRuntimeOption>,
    /// Caches the computed closure per script set + governing-manifest hash so a
    /// generator's read set is not recomputed on every call.
    closure_cache: ClosureCache,
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
        let factory: RunnerFactory = Box::new(
            move |runtime, authorizer, mut spawn_policy, shim_json| {
                let sys = sys.clone();
                let context_dir = context_dir.clone();
                let version = version.clone();

                Box::pin(async move {
                    let vendored =
                        VendoredBridgeService::new(version, None::<String>)
                            .ensure(&context_dir)
                            .await
                            .map_err(|e| Error::custom(e.to_string()))?;

                    // On Windows the confined child is granted only a minimal
                    // boot set — ordinary policy fs is broker-mediated, not
                    // lowered into ACEs — so the vendored bundle root must be
                    // granted explicitly or the runtime cannot read its own
                    // entrypoint to start. On Linux/macOS the bundle already
                    // sits under the granted workspace subtree, so no extra
                    // grant is needed and their spec is left untouched.
                    #[cfg(target_os = "windows")]
                    if let Some(spec) = spawn_policy.os_sandbox.as_mut() {
                        spec.read_paths.push(vendored.root.clone());
                    }

                    let mut router = Router::new();
                    // Enforced: broker every mediated fs operation against the
                    // generator's effective policy before it touches `sys`, and
                    // filter `env` by that same policy (`EnvAccess::Filter` is
                    // the default), so the script's `proc.env()` snapshot only
                    // ever sees policy-allowed variables.
                    let enforcing = PolicyEnforcingSys::new(sys, authorizer);

                    // Scrub the child's ambient environment down to the
                    // policy-allowed snapshot (read through `sys` and filtered by
                    // the very same authorizer the broker uses). Without this the
                    // runtime would inherit omni's *entire* environment and a
                    // script could read any variable through the un-mediated
                    // `process.env` / `Deno.env`, bypassing the `env` capability
                    // on runtimes with no env launch-flag (Node/Bun). The
                    // spawner adds the fixed runtime bootstrap set on top; every
                    // other ambient variable is dropped.
                    spawn_policy.env = Some(enforcing.env_snapshot());

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
                        .with_script_args(&script_args)
                        .with_call_timeout(exec_timeout_from_env()),
                    )
                    .await
                    .map_err(|e| Error::custom(e.to_string()))
                })
            },
        );

        Self {
            pool: RunnerPool::new(),
            scan_pool: RunnerPool::new(),
            closure_cache: ClosureCache::new(),
            factory,
        }
    }

    /// Shuts down every runner that was started. Best-effort.
    pub async fn shutdown(&self) {
        self.pool.shutdown().await;
        self.scan_pool.shutdown().await;
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

        // Enforcement is gated on the experimental capabilities feature. When
        // the workspace has opted in, the declared policy is planned and
        // enforced (a generator that declares none is confined to the built-in
        // floor). When it has not, the script runs unconfined: an
        // allow-everything spawn plan and a pass-through broker.
        let (spawn_policy, shim_policy, mut diagnostics) = if policy.enforce {
            build_spawn_plan(resolved, policy, SpawnPosture::from_env())?
        } else {
            // The feature is off. If the run nonetheless declares a capability
            // policy, surface a warning so it is not silently ignored.
            let mut diagnostics = Vec::new();
            if policy.levels.iter().any(|level| !level.is_empty()) {
                diagnostics.push(diagnostic_event!(
                    DiagnosticLevel::Warn,
                    "a capability policy is declared but the capabilities \
                     feature is experimental and disabled; it is ignored and \
                     scripts run unconfined — enable it with \
                     `enable_experimental: true` (or `enable_experimental: \
                     {{ capabilities: true }}`) in the workspace configuration",
                ));
            }
            (
                build_unconfined_plan(resolved),
                ShimPolicy::new(),
                diagnostics,
            )
        };
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
        let authorizer = if policy.enforce {
            EvaluatingAuthorizer::layered(
                policy.effective_levels(),
                roots,
                policy.context.clone(),
            )
        } else {
            EvaluatingAuthorizer::layered(
                vec![unconfined_authorizer_chain()],
                roots,
                policy.context.clone(),
            )
        };

        let key = (resolved, policy.fingerprint());
        let factory = &self.factory;
        let runner = self
            .pool
            .get_or_try_init(key, move || {
                factory(resolved, authorizer, spawn_policy, shim_json)
            })
            .await?;

        // Grant the confined child read access to exactly the files it will
        // load (the resolved import closure), held only across the `call` that
        // makes it read them and revoked immediately after. Only a real OS
        // sandbox (a Windows AppContainer child) needs this; off Windows and
        // for an unconfined child the grant is a no-op, so the (unconfined)
        // closure scan is skipped entirely rather than paying to spawn a second
        // runtime for nothing.
        let read_scope = if runner.is_confined() {
            let scan_factory = &self.factory;
            let scan_context = policy.context.clone();
            let scan_runner = self
                .scan_pool
                .get_or_try_init(resolved, move || {
                    let scan_spawn = build_scan_plan(resolved);
                    let scan_authorizer = EvaluatingAuthorizer::layered(
                        vec![scan_authorizer_chain()],
                        PathRoots::new(),
                        scan_context,
                    );
                    scan_factory(
                        resolved,
                        scan_authorizer,
                        scan_spawn,
                        String::new(),
                    )
                })
                .await?;

            let entries: Vec<PathBuf> = invocations
                .iter()
                .map(|inv| PathBuf::from(&inv.path))
                .collect();
            let workspace_root =
                policy.roots.base(Root::Workspace).map(Path::to_path_buf);
            let manifests: Vec<PathBuf> = entries
                .iter()
                .flat_map(|entry| {
                    let stop = workspace_root
                        .as_deref()
                        .or_else(|| entry.parent())
                        .unwrap_or(entry.as_path());
                    governing_manifests(entry, stop)
                })
                .collect();

            let closure = self
                .closure_cache
                .get_or_compute(&entries, &manifests, || {
                    scan_closure(&scan_runner, &entries)
                })
                .await?;

            for note in &closure.diagnostics {
                diagnostics.push(diagnostic_event!(
                    DiagnosticLevel::Warn,
                    "import-scan: {note}",
                ));
            }

            runner.grant_read_scope(&closure.paths)
        } else {
            runner.grant_read_scope(&[])
        };

        runner
            .call(EXEC_GENERATOR_SCRIPT_PATH, invocations)
            .await
            .map_err(|e| Error::custom(e.to_string()))?;
        drop(read_scope);

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
            enforce: true,
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
        let (_spawn, _shim, diagnostics) = build_spawn_plan(
            DelegatingJsRuntimeOption::Bun,
            &policy,
            SpawnPosture::default(),
        )
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
        let err = build_spawn_plan(
            DelegatingJsRuntimeOption::Bun,
            &policy,
            SpawnPosture::default(),
        )
        .expect_err("require-floor must refuse an unfloored governed domain");
        // Surfaced as the generator-level enforcement error.
        assert!(
            format!("{err}").contains("cannot enforce"),
            "unexpected error: {err}"
        );
    }

    fn fs_policy(ctx: GeneratorContext) -> EffectivePolicy {
        let chain: CapabilityRules<Generator> = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        )
        .expect("valid fs chain");
        EffectivePolicy {
            levels: vec![chain],
            roots: PathRoots::new().with(Root::Workspace, "/repo"),
            context: ctx,
            strictness: CapabilitiesStrictness::Warn,
            enforce: true,
        }
    }

    #[test]
    fn os_sandbox_disabled_emits_a_loud_downgrade_warning() {
        let policy = fs_policy(GeneratorContext::default());

        // Hatch unset: the OS-sandbox floor is intact, so no downgrade warning.
        let (_spawn, _shim, before) = build_spawn_plan(
            DelegatingJsRuntimeOption::Node,
            &policy,
            SpawnPosture {
                os_sandbox_disabled: false,
            },
        )
        .expect("planning succeeds under the warn stance");
        assert!(
            !before
                .iter()
                .any(|d| d.message.contains("OS-level sandbox is DISABLED")),
            "must not warn about a disabled sandbox when the hatch is \
             unset, got: {before:?}"
        );

        // Escape hatch active: a distinct, prominent downgrade warning appears.
        let (_spawn, _shim, after) = build_spawn_plan(
            DelegatingJsRuntimeOption::Node,
            &policy,
            SpawnPosture {
                os_sandbox_disabled: true,
            },
        )
        .expect("the warn stance never refuses on a floor gap");
        assert!(
            after
                .iter()
                .any(|d| matches!(d.level, DiagnosticLevel::Warn)
                    && d.message.contains("OS-level sandbox is DISABLED")
                    && d.message.contains("OMNI_DISABLE_OS_SANDBOX")),
            "expected a loud OS-sandbox-disabled warning when the hatch is \
             set, got: {after:?}"
        );
    }

    #[test]
    fn fingerprint_is_stable_for_identical_policies() {
        let a = fs_policy(GeneratorContext {
            action: Some("build".into()),
            target: None,
        });
        let b = fs_policy(GeneratorContext {
            action: Some("build".into()),
            target: None,
        });
        assert_eq!(
            a.fingerprint(),
            b.fingerprint(),
            "identical effective policies must share a process"
        );
    }

    #[test]
    fn fingerprint_distinguishes_the_evaluation_context() {
        // Two actions with identical levels + roots but a different action
        // context must NOT collide: `applies_to.actions`-scoped rules resolve
        // against the context, so sharing a confined process would let one
        // action run under the other's context-scoped authority (the cross-
        // action authority-bleed this key omission used to allow).
        let deploy = fs_policy(GeneratorContext {
            action: Some("deploy".into()),
            target: None,
        });
        let build = fs_policy(GeneratorContext {
            action: Some("build".into()),
            target: None,
        });
        assert_ne!(
            deploy.fingerprint(),
            build.fingerprint(),
            "different action contexts must not share a process"
        );

        // The target half of the context is likewise part of the identity.
        let src = fs_policy(GeneratorContext {
            action: Some("build".into()),
            target: Some("src".into()),
        });
        let docs = fs_policy(GeneratorContext {
            action: Some("build".into()),
            target: Some("docs".into()),
        });
        assert_ne!(src.fingerprint(), docs.fingerprint());
    }

    #[test]
    fn fingerprint_distinguishes_the_floor_strictness() {
        // Same levels/roots/context, different floor stance: the process was
        // planned under a specific strictness, so the two must not be shared.
        let mut warn = fs_policy(GeneratorContext::default());
        warn.strictness = CapabilitiesStrictness::Warn;
        let mut strict = fs_policy(GeneratorContext::default());
        strict.strictness = CapabilitiesStrictness::RequireFloor;
        assert_ne!(warn.fingerprint(), strict.fingerprint());
    }

    #[test]
    fn scan_plan_is_unconfined_and_grants_deno_what_deno_info_needs() {
        // The import-scan runner must never install an OS sandbox: it is trusted
        // tooling that reads across the tree to resolve imports.
        for runtime in [
            DelegatingJsRuntimeOption::Node,
            DelegatingJsRuntimeOption::Bun,
            DelegatingJsRuntimeOption::Deno,
        ] {
            let plan = build_scan_plan(runtime);
            assert!(
                plan.os_sandbox.is_none(),
                "the scan runner is unconfined ({runtime:?})"
            );
        }

        // Node/Bun have no pre-spawn permission model, so no flags are needed.
        assert!(
            build_scan_plan(DelegatingJsRuntimeOption::Node)
                .args
                .is_empty()
        );
        assert!(
            build_scan_plan(DelegatingJsRuntimeOption::Bun)
                .args
                .is_empty()
        );

        // Deno enforces its own model, so the scan process needs read access
        // and permission to spawn `deno info`.
        let deno = build_scan_plan(DelegatingJsRuntimeOption::Deno);
        assert!(deno.args.iter().any(|a| a == "--allow-read"), "{deno:?}");
        assert!(deno.args.iter().any(|a| a == "--allow-run"), "{deno:?}");
    }

    #[test]
    fn scan_authorizer_allows_every_env_name() {
        // The scan runtime inherits the ambient env (so `deno info`/the resolver
        // see PATH, HOME, DENO_DIR, …): every env name must authorize under the
        // permissive scan chain, which is what makes `env_snapshot` pass the
        // full environment through rather than filtering it.
        use omni_capabilities::Access;
        let chain = scan_authorizer_chain();
        let env_allows_all = chain.iter().any(|cap| {
            cap.rule.access == Access::Allow
                && cap.rule.domain == CapabilityDomain::Env
                && cap.rule.patterns.iter().any(|p| p == "*")
        });
        assert!(
            env_allows_all,
            "the scan chain must allow all env names: {chain:?}"
        );
    }

    // NOTE: the live "closure granted before the call, revoked after" behaviour
    // requires a real confined AppContainer child and is exercised by the
    // Windows confined e2e in the `@omni-oss/omni-tests` package (this crate
    // keeps no live-spawn unit tests, matching `bridge_rpc_runner`). The
    // "Node grants an empty script closure" property only holds once broker-
    // served module loading (strategy A) removes the per-generator disk reads;
    // until then Node reads its scripts from disk and the closure is non-empty.
}
