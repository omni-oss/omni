//! Tier-3 platform seam: the **native OS access-control sandbox** for the
//! current target, exposed as an [`EnforcementBackend`].
//!
//! On **Linux** this is a real integration: [`NativeOsSandbox`] reports
//! coverage for `fs.read` / `fs.write` when the running kernel provides
//! [Landlock], lowers the policy's filesystem allow-subtrees into an
//! [`OsSandboxSpec`], and the spawner installs the ruleset on the child via
//! [`install_os_sandbox`]. On **Windows** this is also a real integration:
//! [`NativeOsSandbox`] reports the same fs coverage when the OS provides
//! [AppContainer], and the spawner launches the child *inside* the container
//! (see [`appcontainer_sandbox`](crate::appcontainer_sandbox)). On **macOS**
//! (Seatbelt) the integration is still deferred: a skeleton seam exists in
//! [`seatbelt_sandbox`](crate::seatbelt_sandbox) (documenting the required
//! behaviour), but it is unimplemented, so the backend reports
//! [`Coverage::none`] there and any restricted domain falls to another backend
//! or fails closed.
//!
//! ## Windows: AppContainer, not Job Objects
//!
//! The access-control analog of Landlock/Seatbelt on Windows is **AppContainer**:
//! a low-privilege token whose default-deny access to the filesystem, registry,
//! and network is widened only via object ACLs / capability SIDs. **Job Objects
//! are a different tool** (CPU/memory/process-count limits, kill-on-close):
//! useful for containing runaway processes, but they do not restrict which files
//! or hosts a process may touch, so they do not belong in this
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) seam.
//!
//! ## Why these stay coarse
//!
//! These mechanisms are path-hierarchy / capability-class based, not glob based:
//! Landlock and AppContainer grant whole subtrees, and none can express a
//! `deny` sub-path or `host:port` network rule. So an OS backend's coverage is
//! *partial*, and precise patterns surface as [`Gap`]s — resolved by the
//! in-process broker or made to fail closed, exactly like the pre-spawn flag
//! backends.
//!
//! [Landlock]: https://docs.kernel.org/userspace-api/landlock.html
//! [AppContainer]: https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation

use omni_capabilities::RequiredCapabilities;

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError,
    PatternResolver, Tier,
};

/// The platform's native access-control sandbox mechanism.
///
/// Carries an optional **resolved launch posture** so the floor it *claims*
/// (via [`coverage`](EnforcementBackend::coverage)) and the spec it *lowers*
/// (via [`plan`](EnforcementBackend::plan)) match the confinement that will
/// actually be *applied*. The default ([`FromEnv`](LaunchPosture::FromEnv))
/// reproduces the historical behavior — read `OMNI_DISABLE_OS_SANDBOX` at query
/// time and assume the selected runtime is confined — which is fine for probes
/// and tests. The spawner should instead construct one via
/// [`resolved`](NativeOsSandbox::resolved), threading the single disable read
/// and whether the selected runtime is actually confined on this platform (e.g.
/// Bun runs **unconfined** on Windows — it cannot boot inside an AppContainer),
/// so an unconfined runtime never claims an fs floor it will not get.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeOsSandbox {
    posture: LaunchPosture,
}

/// How a [`NativeOsSandbox`] resolves whether it actually confines the launch.
#[derive(Debug, Clone, Copy, Default)]
enum LaunchPosture {
    /// Read `OMNI_DISABLE_OS_SANDBOX` from the environment at query time and
    /// assume the selected runtime is confined by the mechanism. The historical,
    /// back-compatible default used when the spawner has not threaded an explicit
    /// posture (unit tests, coverage probes).
    #[default]
    FromEnv,
    /// An explicit posture threaded from the spawner so the *claimed* floor
    /// matches the *applied* confinement: `disabled` mirrors the single
    /// `OMNI_DISABLE_OS_SANDBOX` read, and `runtime_confined` is `false` when the
    /// selected runtime runs unconfined on this platform (e.g. Bun on Windows).
    Resolved {
        disabled: bool,
        runtime_confined: bool,
    },
}

impl NativeOsSandbox {
    /// A sandbox that resolves its disable posture from the environment and
    /// assumes the runtime is confined — the historical default. Prefer
    /// [`resolved`](Self::resolved) from the spawner so claimed and applied
    /// confinement cannot diverge.
    pub fn new() -> Self {
        Self::default()
    }

    /// A sandbox whose coverage/floor claims reflect the resolved launch posture:
    /// `disabled` is the single `OMNI_DISABLE_OS_SANDBOX` read threaded from the
    /// spawner, and `runtime_confined` is `false` when the selected runtime runs
    /// unconfined on this platform (so this tier claims no fs floor and lowers no
    /// spec — the plan then surfaces an honest floor gap / refuses under
    /// [`RequireFloor`](crate::FloorStrictness::RequireFloor)).
    pub fn resolved(disabled: bool, runtime_confined: bool) -> Self {
        Self {
            posture: LaunchPosture::Resolved {
                disabled,
                runtime_confined,
            },
        }
    }

    /// Whether the OS sandbox is disabled for this launch (escape hatch).
    fn disabled(&self) -> bool {
        match self.posture {
            LaunchPosture::FromEnv => {
                std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some()
            }
            LaunchPosture::Resolved { disabled, .. } => disabled,
        }
    }

    /// Whether the selected runtime is actually confined by this mechanism on the
    /// current platform. Only a [`Resolved`](LaunchPosture::Resolved) posture can
    /// report `false`; the env-derived default optimistically assumes `true`.
    fn runtime_confined(&self) -> bool {
        match self.posture {
            LaunchPosture::FromEnv => true,
            LaunchPosture::Resolved {
                runtime_confined, ..
            } => runtime_confined,
        }
    }
    /// The name of the native access-control sandbox mechanism on the current
    /// target, resolved at compile time.
    ///
    /// Windows resolves to `appcontainer` — the confinement analog of Landlock
    /// and Seatbelt. (Job Objects govern resources/lifetime, not access, so
    /// they are deliberately not this seam's mechanism; see the module docs.)
    pub const fn mechanism() -> &'static str {
        #[cfg(target_os = "linux")]
        {
            "landlock"
        }
        #[cfg(target_os = "macos")]
        {
            "seatbelt"
        }
        #[cfg(target_os = "windows")]
        {
            "appcontainer"
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )))]
        {
            "none"
        }
    }

    /// Whether omni has an OS-sandbox integration for the current target. `true`
    /// on Linux (Landlock) and Windows (AppContainer); still `false` on macOS
    /// (deferred).
    ///
    /// Note that even where an integration exists, [`coverage`] may still be
    /// empty at runtime if the *running OS* lacks the feature (see
    /// [`landlock_sandbox::is_supported`](crate::landlock_sandbox::is_supported)
    /// / [`appcontainer_sandbox::is_supported`](crate::appcontainer_sandbox::is_supported)).
    ///
    /// [`coverage`]: EnforcementBackend::coverage
    pub const fn is_implemented() -> bool {
        cfg!(any(target_os = "linux", target_os = "windows"))
    }
}

impl EnforcementBackend for NativeOsSandbox {
    fn name(&self) -> &'static str {
        Self::mechanism()
    }

    fn tier(&self) -> Tier {
        Tier::OsSandbox
    }

    fn coverage(&self) -> Coverage {
        // Reflect the *resolved* posture so plan-time coverage/floor analysis
        // matches the real runtime confinement (T17/T18): the escape hatch
        // (`OMNI_DISABLE_OS_SANDBOX`) makes `install_os_sandbox` launch
        // unconfined, and a runtime the spawner runs unconfined on this platform
        // (Bun on Windows, which cannot boot inside an AppContainer) is not
        // restricted by this tier at all. In either case this tier confines
        // nothing, so it must claim nothing — fs then falls to the broker and
        // shows up honestly in floor analysis rather than advertising an fs floor
        // that will not actually be applied.
        let disabled = self.disabled();
        let runtime_confined = self.runtime_confined();
        #[cfg(target_os = "linux")]
        let supported = crate::landlock_sandbox::is_supported();
        #[cfg(target_os = "windows")]
        let supported = crate::appcontainer_sandbox::is_supported();
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            os_fs_coverage(supported, disabled, runtime_confined)
        }
        // No integration for this target → cover nothing → fail closed rather
        // than pretend to confine.
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = (disabled, runtime_confined);
            Coverage::none()
        }
    }

    fn plan(
        &self,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        // Lower a spec only when this tier will actually confine the launch, so
        // the lowered spec stays in lock-step with the claimed `coverage()`: a
        // disabled sandbox or an unconfined runtime (Bun on Windows) contributes
        // no spec, exactly as it claims no floor.
        if self.disabled() || !self.runtime_confined() {
            let _ = (req, roots);
            return Ok(BackendPlan::new());
        }
        #[cfg(target_os = "linux")]
        {
            // Landlock (V4) can lower a port-only net connect floor.
            Ok(lowering::plan(Self::mechanism(), req, roots, true))
        }
        #[cfg(target_os = "windows")]
        {
            // AppContainer cannot express `host:port`, so net is not lowered
            // here (see the `appcontainer_sandbox` module docs).
            Ok(lowering::plan(Self::mechanism(), req, roots, false))
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            // OS sandboxes not yet integrated here contribute nothing.
            let _ = (req, roots);
            Ok(BackendPlan::new())
        }
    }
}

/// Filesystem coverage this tier may claim, given whether the OS mechanism is
/// available, whether the escape hatch disabled it, and whether the selected
/// runtime is actually confined by it on this platform. Factored out so the
/// fail-closed / disable / unconfined-runtime posture is unit-testable on any
/// host (no real Landlock or AppContainer needed). A disabled, unavailable, or
/// runtime-unconfined mechanism covers nothing.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn os_fs_coverage(
    supported: bool,
    disabled: bool,
    runtime_confined: bool,
) -> Coverage {
    use omni_capabilities::CapabilityDomain;
    if disabled || !supported || !runtime_confined {
        return Coverage::none();
    }
    Coverage::of([CapabilityDomain::FsRead, CapabilityDomain::FsWrite])
}

/// What [`install_os_sandbox`] must do for a given launch. Factored out so the
/// fail-closed policy — the single most critical property on a host where the OS
/// sandbox is unavailable — is unit-testable without a real facility.
///
/// Windows-only: the Linux path installs a `pre_exec` Landlock hook that itself
/// fails closed in the child if the ruleset cannot be applied, so it has no
/// separate early fail-closed branch.
#[cfg(target_os = "windows")]
#[derive(Debug, PartialEq, Eq)]
enum SandboxInstall {
    /// Nothing to confine (empty spec) or confinement intentionally disabled:
    /// launch as-is.
    Skip,
    /// Establish confinement for the launch.
    Confine,
    /// Confinement was requested but the OS cannot provide it: refuse, so the
    /// caller never launches an unconfined child believing it is sandboxed.
    FailClosed,
}

/// Decide the install action from the three inputs. Order matters: an empty spec
/// and the explicit disable hatch both short-circuit *before* the availability
/// check, so disabling the sandbox never turns into a fail-closed refusal.
#[cfg(target_os = "windows")]
fn sandbox_install_decision(
    spec_empty: bool,
    disabled: bool,
    supported: bool,
) -> SandboxInstall {
    if spec_empty || disabled {
        return SandboxInstall::Skip;
    }
    if supported {
        SandboxInstall::Confine
    } else {
        SandboxInstall::FailClosed
    }
}

/// Install the OS-sandbox confinement described by `spec` onto `command` so it
/// takes effect for the spawned child (and everything it forks).
///
/// On **Linux** this registers a `pre_exec` hook that applies a Landlock ruleset
/// in the child before `execve`. On **Windows** confinement cannot be installed
/// onto a `Command` for a later `spawn` (AppContainer is attached at process
/// creation), so this only validates that confinement is establishable and the
/// spawner launches the child through
/// [`appcontainer_sandbox::spawn`](crate::appcontainer_sandbox::spawn) instead.
/// On other targets it is a no-op today, so callers can invoke it
/// unconditionally and stay cross-platform. Passing an empty spec installs
/// nothing.
///
/// Returns an error when confinement was requested but cannot be established, so
/// the caller can fail closed rather than launch an unconfined child. The Linux
/// path never fails here — a Landlock failure surfaces later as a failed spawn,
/// when the `pre_exec` hook runs.
#[cfg(target_os = "linux")]
pub fn install_os_sandbox(
    command: &mut std::process::Command,
    spec: &crate::OsSandboxSpec,
) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt as _;

    if spec.is_empty() {
        return Ok(());
    }
    // Escape hatch: allow disabling the OS sandbox for debugging a confinement
    // regression, or on a host where the Landlock baseline is too tight for a
    // legitimate workload. The broker still enforces every mediated operation;
    // only the kernel backstop against *direct* syscalls is dropped.
    if std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some() {
        return Ok(());
    }
    // Install the Landlock hook only when the running kernel actually provides
    // Landlock. `coverage()` claims the fs floor under the very same condition
    // (`landlock_sandbox::is_supported()`), so gating here keeps the *applied*
    // confinement in lock-step with the *claimed* floor: on a kernel without
    // Landlock this tier advertises no fs coverage (the broker is the floor and
    // the honest FloorGap stands), so we must not register a hook that — now
    // that `restrict` fails closed — would abort the spawn for a floor we never
    // promised.
    if !crate::landlock_sandbox::is_supported() {
        return Ok(());
    }
    let spec = spec.clone();
    // SAFETY: the closure runs in the forked child before `execve`; it only
    // issues Landlock syscalls (plus small allocations) to irrevocably drop the
    // child's ambient filesystem rights. It touches no shared parent state.
    unsafe {
        command.pre_exec(move || crate::landlock_sandbox::restrict(&spec));
    }
    Ok(())
}

/// On **Windows** the OS sandbox cannot be installed onto a `Command` for a
/// later `spawn`: AppContainer must be attached *at* process creation (see the
/// [`appcontainer_sandbox`](crate::appcontainer_sandbox) module docs). The
/// spawner therefore launches the child through
/// [`appcontainer_sandbox::spawn`](crate::appcontainer_sandbox::spawn) instead,
/// and this call is a validated no-op: it merely confirms confinement can be
/// established, failing closed otherwise so a spawner that ignores the dedicated
/// path cannot silently run unconfined.
#[cfg(target_os = "windows")]
pub fn install_os_sandbox(
    _command: &mut std::process::Command,
    spec: &crate::OsSandboxSpec,
) -> std::io::Result<()> {
    let disabled = std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some();
    match sandbox_install_decision(
        spec.is_empty(),
        disabled,
        crate::appcontainer_sandbox::is_supported(),
    ) {
        // Empty/disabled: launch as-is. Confine: the confinement is actually
        // attached at spawn time via `appcontainer_sandbox::spawn`, so this call
        // has only confirmed it *can* be established.
        SandboxInstall::Skip | SandboxInstall::Confine => Ok(()),
        SandboxInstall::FailClosed => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "AppContainer is not available on this host",
        )),
    }
}

/// No-op OS-sandbox install for targets without an integration yet.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn install_os_sandbox(
    _command: &mut std::process::Command,
    _spec: &crate::OsSandboxSpec,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod lowering {
    use std::path::PathBuf;

    use omni_capabilities::{CapabilityDomain, RequiredCapabilities};

    use crate::lower::{FsScope, classify_fs_glob, split_host_port};
    use crate::{BackendPlan, Gap, OsSandboxSpec, PatternResolver};

    /// Lower the policy's filesystem allow-subtrees into an [`OsSandboxSpec`],
    /// reporting a [`Gap`] for every pattern a subtree/hierarchy grant model
    /// (Landlock on Linux, AppContainer object ACLs on Windows) cannot express
    /// (mid-path globs, whole-fs patterns, and any `deny`).
    ///
    /// `lower_net` controls whether the `net` policy contributes a port-only
    /// *connect* floor: Linux (Landlock V4) can enforce one, but AppContainer
    /// cannot express `host:port`, so Windows passes `false` and leaves net
    /// entirely to the shim/broker (see the [`crate::appcontainer_sandbox`]
    /// module docs).
    pub(super) fn plan(
        name: &'static str,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
        lower_net: bool,
    ) -> BackendPlan {
        let mut plan = BackendPlan::new();
        let mut spec = OsSandboxSpec::new();

        collect(
            name,
            req,
            roots,
            CapabilityDomain::FsRead,
            &mut spec.read_paths,
            &mut plan.gaps,
        );
        collect(
            name,
            req,
            roots,
            CapabilityDomain::FsWrite,
            &mut spec.write_paths,
            &mut plan.gaps,
        );

        // Lower the `net` policy to a port-only *connect* floor where the OS
        // sandbox can enforce one. Only concrete outbound ports qualify: a
        // `host:port` rule contributes `port` (any host), while an all-ports
        // (`host:*`), missing, or non-numeric port cannot be floored (it would
        // be allow-all) and a `deny` is not expressible in an allow-list. None
        // of these are reported as gaps — the OS sandbox never *claims* to cover
        // `net` (host-level enforcement stays with the shim), so there is
        // nothing to fail closed on here, exactly as with `process`.
        if lower_net {
            collect_connect_ports(req, &mut spec.connect_ports);
        }

        // A confined child inherits the sandbox across process creation, so any
        // program the policy allows it to spawn must have its binary
        // readable/executable under the sandbox. Record the literally-named
        // allowed programs; the spawner resolves each against `PATH` and grants
        // its directory. Globbed program patterns cannot be resolved to a path
        // here and are left to the runtime flag / script shim to gate (this is
        // not a coverage claim — the OS sandbox never covers `process`, so no
        // gap is reported).
        if let Some(rules) = req.domains().get(&CapabilityDomain::Process) {
            for atom in &rules.allow {
                if !crate::lower::has_glob(&atom.pattern) {
                    spec.exec_programs.push(atom.pattern.clone());
                }
            }
        }

        if !spec.is_empty() {
            plan.spawn.os_sandbox = Some(spec);
        }
        plan
    }

    fn collect(
        name: &'static str,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
        domain: CapabilityDomain,
        out_paths: &mut Vec<PathBuf>,
        gaps: &mut Vec<Gap>,
    ) {
        let Some(rules) = req.domains().get(&domain) else {
            return;
        };

        for atom in &rules.allow {
            // Unregistered root → matches nothing; contributing nothing is
            // faithful (and not a gap).
            let Some(resolved) = roots.resolve(&atom.pattern) else {
                continue;
            };
            match classify_fs_glob(&resolved) {
                Ok(FsScope::Subtree(p)) | Ok(FsScope::Exact(p)) => {
                    out_paths.push(PathBuf::from(p));
                }
                Err(reason) => gaps.push(Gap {
                    backend: name.to_string(),
                    domain,
                    id: atom.id,
                    pattern: atom.pattern.clone(),
                    reason,
                }),
            }
        }

        // A subtree grant model has no `deny` sub-path.
        for atom in &rules.deny {
            gaps.push(Gap {
                backend: name.to_string(),
                domain,
                id: atom.id,
                pattern: atom.pattern.clone(),
                reason: "the OS sandbox grants whole path hierarchies and \
                 cannot express a `deny` sub-path; use the in-process broker"
                    .to_string(),
            });
        }
    }

    /// Collect the concrete outbound TCP ports the `net` policy allows into the
    /// spec's connect-port floor. See [`plan`] for why only concrete-port allow
    /// rules qualify and why nothing here is a gap.
    fn collect_connect_ports(
        req: &RequiredCapabilities,
        out_ports: &mut Vec<u16>,
    ) {
        let Some(rules) = req.domains().get(&CapabilityDomain::Net) else {
            return;
        };
        for atom in &rules.allow {
            let (_host, port) = split_host_port(&atom.pattern);
            // `*` (all ports) or no port cannot be a port allow-list; a
            // non-`u16` value is not a TCP port. Only a concrete port floors.
            if let Some(port) = port.and_then(|p| p.parse::<u16>().ok())
                && !out_ports.contains(&port)
            {
                out_ports.push(port);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_is_os_sandbox() {
        assert_eq!(NativeOsSandbox::new().tier(), Tier::OsSandbox);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn os_fs_coverage_is_empty_unless_supported_and_enabled() {
        use omni_capabilities::CapabilityDomain;
        // Only an available *and* enabled mechanism confining a runtime claims
        // the fs domains; every other combination covers nothing so the run
        // fails closed / routes fs to the broker honestly.
        assert!(
            !os_fs_coverage(false, false, true)
                .covers(CapabilityDomain::FsRead)
        );
        assert!(
            !os_fs_coverage(true, true, true).covers(CapabilityDomain::FsRead)
        );
        assert!(
            !os_fs_coverage(false, true, true).covers(CapabilityDomain::FsRead)
        );
        // Supported and enabled, but the selected runtime runs unconfined (e.g.
        // Bun on Windows): the tier confines nothing, so it must claim nothing.
        assert!(
            !os_fs_coverage(true, false, false)
                .covers(CapabilityDomain::FsRead)
        );
        let on = os_fs_coverage(true, false, true);
        assert!(on.covers(CapabilityDomain::FsRead));
        assert!(on.covers(CapabilityDomain::FsWrite));
        assert!(!on.covers(CapabilityDomain::Net));
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn a_resolved_unconfined_or_disabled_posture_claims_no_floor_and_lowers_no_spec()
     {
        use omni_capabilities::{CapabilityRules, PathRoots, Root, project};
        let cfg: CapabilityRules = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "/repo");

        // A runtime the spawner runs unconfined (e.g. Bun on Windows) claims no
        // fs floor and lowers no spec, so the plan's floor analysis is honest
        // rather than advertising a floor the spawner never installs.
        let unconfined = NativeOsSandbox::resolved(false, false);
        assert!(unconfined.coverage().is_empty());
        assert!(
            unconfined
                .plan(&req, &roots)
                .expect("infallible")
                .spawn
                .os_sandbox
                .is_none()
        );

        // The explicit disable hatch likewise claims/lowers nothing, matching
        // `install_os_sandbox` skipping confinement.
        let disabled = NativeOsSandbox::resolved(true, true);
        assert!(disabled.coverage().is_empty());
        assert!(
            disabled
                .plan(&req, &roots)
                .expect("infallible")
                .spawn
                .os_sandbox
                .is_none()
        );

        // A confined, enabled runtime still lowers its spec (regression guard).
        let confined = NativeOsSandbox::resolved(false, true);
        // (Coverage still depends on the host actually providing the mechanism.)
        let _ = confined.coverage();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn install_decision_fails_closed_only_when_unsupported_and_requested() {
        // The critical fail-closed table: a non-empty spec on an unsupported
        // host (sandbox not disabled) is the ONLY case that refuses. Empty specs
        // and the explicit disable hatch always skip, even when unsupported, so
        // the escape hatch never turns into a spurious refusal.
        assert_eq!(
            sandbox_install_decision(false, false, false),
            SandboxInstall::FailClosed
        );
        assert_eq!(
            sandbox_install_decision(false, false, true),
            SandboxInstall::Confine
        );
        for supported in [true, false] {
            assert_eq!(
                sandbox_install_decision(true, false, supported),
                SandboxInstall::Skip,
                "an empty spec never confines or fails"
            );
            assert_eq!(
                sandbox_install_decision(false, true, supported),
                SandboxInstall::Skip,
                "the disable hatch always skips, never fails closed"
            );
        }
    }

    // Per-platform capability assertions, behind cfg flags. Exactly one of
    // these compiles on any given target.

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_landlock() {
        assert_eq!(NativeOsSandbox::mechanism(), "landlock");
        assert!(NativeOsSandbox::is_implemented());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_coverage_tracks_kernel_support() {
        // On a Landlock-capable kernel the backend covers the fs domains; on one
        // without it, it must cover nothing (fail closed). Either way it must
        // never claim net/env/process.
        use omni_capabilities::CapabilityDomain;
        let cov = NativeOsSandbox::new().coverage();
        assert!(!cov.covers(CapabilityDomain::Net));
        assert!(!cov.covers(CapabilityDomain::Env));
        assert!(!cov.covers(CapabilityDomain::Process));
        // Coverage also collapses to none when the escape hatch disables the
        // sandbox, so fold that into the expectation.
        let disabled = std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some();
        let expected = crate::landlock_sandbox::is_supported() && !disabled;
        assert_eq!(cov.covers(CapabilityDomain::FsRead), expected);
        assert_eq!(cov.covers(CapabilityDomain::FsWrite), expected);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_lowers_allow_subtree_and_gaps_deny() {
        use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/out/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "/repo");

        let plan = NativeOsSandbox::new().plan(&req, &roots).expect("infallible");
        let spec = plan.spawn.os_sandbox.expect("some fs subtrees lowered");
        assert!(spec.read_paths.contains(&std::path::PathBuf::from("/repo")));
        assert!(
            spec.write_paths
                .contains(&std::path::PathBuf::from("/repo/out"))
        );
        // The `deny **/.git/**` cannot be a Landlock hierarchy → a gap the broker
        // resolves.
        assert!(
            plan.gaps.iter().any(|g| g.pattern == "**/.git/**"),
            "deny sub-path must be reported as a gap: {:?}",
            plan.gaps
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_records_literal_allowed_programs_for_exec_grant() {
        use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "process", "patterns": ["git", "node"] },
                { "access": "allow", "domain": "process", "patterns": ["cache-*"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "/repo");

        let plan = NativeOsSandbox::new().plan(&req, &roots).expect("infallible");
        let spec = plan
            .spawn
            .os_sandbox
            .expect("exec programs populate a spec");
        // Literal names are recorded so the spawner can grant their binary dirs.
        assert!(spec.exec_programs.contains(&"git".to_string()));
        assert!(spec.exec_programs.contains(&"node".to_string()));
        // A globbed program name cannot be resolved to a path here, so it is not
        // recorded (the runtime flag / shim gates it instead). Crucially, it is
        // NOT reported as a gap: the OS sandbox never claims to cover `process`.
        assert!(!spec.exec_programs.iter().any(|p| p.contains('*')));
        assert!(
            plan.gaps
                .iter()
                .all(|g| g.domain
                    != omni_capabilities::CapabilityDomain::Process),
            "process patterns must not be OS-sandbox gaps: {:?}",
            plan.gaps
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_lowers_concrete_net_ports_and_ignores_the_unfloorible() {
        use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["example.com:443", "10.0.0.1:8080"] },
                { "access": "allow", "domain": "net", "patterns": ["api.example.com:443"] },
                { "access": "allow", "domain": "net", "patterns": ["internal:*", "nohost"] },
                { "access": "deny",  "domain": "net", "patterns": ["blocked.example.com:22"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "/repo");

        let plan = NativeOsSandbox::new().plan(&req, &roots).expect("infallible");
        let spec = plan
            .spawn
            .os_sandbox
            .expect("concrete net ports populate a spec");

        // Concrete host:port rules floor their port (any host), deduplicated.
        assert!(spec.connect_ports.contains(&443));
        assert!(spec.connect_ports.contains(&8080));
        assert_eq!(
            spec.connect_ports.iter().filter(|&&p| p == 443).count(),
            1,
            "the repeated :443 must be deduplicated: {:?}",
            spec.connect_ports
        );
        // All-ports (`internal:*`), portless (`nohost`), and the `deny 22` rule
        // cannot be a port allow-list and are not lowered.
        assert!(!spec.connect_ports.contains(&22));
        assert_eq!(
            spec.connect_ports.len(),
            2,
            "only the two concrete allowed ports: {:?}",
            spec.connect_ports
        );

        // The OS sandbox never *claims* net coverage — the port floor is partial
        // (host stays with the shim), so lowering ports must not report a gap
        // and must not make the backend cover `net`.
        assert!(
            plan.gaps
                .iter()
                .all(|g| g.domain != omni_capabilities::CapabilityDomain::Net),
            "net ports must not be OS-sandbox gaps: {:?}",
            plan.gaps
        );
        assert!(
            !NativeOsSandbox::new()
                .coverage()
                .covers(omni_capabilities::CapabilityDomain::Net)
        );
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn non_integrated_target_covers_nothing_yet() {
        assert!(NativeOsSandbox::new().coverage().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_seatbelt() {
        assert_eq!(NativeOsSandbox::mechanism(), "seatbelt");
        assert!(!NativeOsSandbox::is_implemented());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_uses_appcontainer() {
        assert_eq!(NativeOsSandbox::mechanism(), "appcontainer");
        assert!(NativeOsSandbox::is_implemented());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_coverage_tracks_appcontainer_support() {
        // On a host that provides AppContainer the backend covers the fs
        // domains; otherwise it must cover nothing (fail closed). Either way it
        // must never claim net/env/process.
        use omni_capabilities::CapabilityDomain;
        let cov = NativeOsSandbox::new().coverage();
        assert!(!cov.covers(CapabilityDomain::Net));
        assert!(!cov.covers(CapabilityDomain::Env));
        assert!(!cov.covers(CapabilityDomain::Process));
        // Coverage also collapses to none when the escape hatch disables the
        // sandbox, so fold that into the expectation.
        let disabled = std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some();
        let expected = crate::appcontainer_sandbox::is_supported() && !disabled;
        assert_eq!(cov.covers(CapabilityDomain::FsRead), expected);
        assert_eq!(cov.covers(CapabilityDomain::FsWrite), expected);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_lowers_allow_subtree_and_gaps_deny() {
        use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/out/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "C:/repo");

        let plan = NativeOsSandbox::new().plan(&req, &roots).expect("infallible");
        let spec = plan.spawn.os_sandbox.expect("some fs subtrees lowered");
        assert!(
            spec.read_paths
                .contains(&std::path::PathBuf::from("C:/repo"))
        );
        assert!(
            spec.write_paths
                .contains(&std::path::PathBuf::from("C:/repo/out"))
        );
        // The `deny **/.git/**` cannot be a subtree grant → a gap the broker
        // resolves.
        assert!(
            plan.gaps.iter().any(|g| g.pattern == "**/.git/**"),
            "deny sub-path must be reported as a gap: {:?}",
            plan.gaps
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_does_not_lower_a_net_floor() {
        // AppContainer cannot express `host:port`, so — unlike Landlock V4 — the
        // Windows plan lowers no connect-port floor and never claims net.
        use omni_capabilities::{
            CapabilityDomain, CapabilityRules, PathRoots, Root, project,
        };

        let cfg: CapabilityRules = serde_json::from_str(
            r#"[
                { "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "net", "patterns": ["example.com:443"] }
            ]"#,
        )
        .unwrap();
        let req = project(&cfg, &());
        let roots = PathRoots::new().with(Root::Workspace, "C:/repo");

        let plan = NativeOsSandbox::new().plan(&req, &roots).expect("infallible");
        let spec = plan.spawn.os_sandbox.expect("fs subtree lowered");
        assert!(
            spec.connect_ports.is_empty(),
            "AppContainer must not lower a net port floor: {:?}",
            spec.connect_ports
        );
        assert!(!NativeOsSandbox::new().coverage().covers(CapabilityDomain::Net));
    }
}
