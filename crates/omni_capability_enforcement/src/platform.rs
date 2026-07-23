//! Tier-3 platform seam: the **native OS access-control sandbox** for the
//! current target, exposed as an [`EnforcementBackend`].
//!
//! On **Linux** this is now a real integration: [`NativeOsSandbox`] reports
//! coverage for `fs.read` / `fs.write` when the running kernel provides
//! [Landlock], lowers the policy's filesystem allow-subtrees into an
//! [`OsSandboxSpec`], and the spawner installs the ruleset on the child via
//! [`install_os_sandbox`]. On **macOS** (Seatbelt) and **Windows**
//! (AppContainer) the integrations are still deferred: skeleton seams exist in
//! [`seatbelt_sandbox`](crate::seatbelt_sandbox) and
//! [`appcontainer_sandbox`](crate::appcontainer_sandbox) (documenting the
//! required behaviour), but they are unimplemented, so the backend reports
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

use omni_capabilities::RequiredCapabilities;

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError,
    PatternResolver, Tier,
};

/// The platform's native access-control sandbox mechanism.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeOsSandbox;

impl NativeOsSandbox {
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
    /// on Linux (Landlock); still `false` on macOS / Windows (deferred).
    ///
    /// Note that even where an integration exists, [`coverage`] may still be
    /// empty at runtime if the *running kernel* lacks the feature (see
    /// [`landlock_sandbox::is_supported`](crate::landlock_sandbox::is_supported)).
    ///
    /// [`coverage`]: EnforcementBackend::coverage
    pub const fn is_implemented() -> bool {
        cfg!(target_os = "linux")
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
        #[cfg(target_os = "linux")]
        {
            use omni_capabilities::CapabilityDomain;
            if crate::landlock_sandbox::is_supported() {
                return Coverage::of([
                    CapabilityDomain::FsRead,
                    CapabilityDomain::FsWrite,
                ]);
            }
        }
        // No integration for this target, or the running kernel lacks it →
        // cover nothing → fail closed rather than pretend to confine.
        Coverage::none()
    }

    fn plan(
        &self,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        #[cfg(target_os = "linux")]
        {
            Ok(linux::plan(Self::mechanism(), req, roots))
        }
        #[cfg(not(target_os = "linux"))]
        {
            // OS sandboxes not yet integrated here contribute nothing.
            let _ = (req, roots);
            Ok(BackendPlan::new())
        }
    }
}

/// Install the OS-sandbox confinement described by `spec` onto `command` so it
/// takes effect for the spawned child (and everything it forks).
///
/// On Linux this registers a `pre_exec` hook that applies a Landlock ruleset in
/// the child before `execve`. On other targets it is a no-op today, so callers
/// can invoke it unconditionally and stay cross-platform. Passing an empty spec
/// installs nothing.
#[cfg(target_os = "linux")]
pub fn install_os_sandbox(
    command: &mut std::process::Command,
    spec: &crate::OsSandboxSpec,
) {
    use std::os::unix::process::CommandExt as _;

    if spec.is_empty() {
        return;
    }
    // Escape hatch: allow disabling the OS sandbox for debugging a confinement
    // regression, or on a host where the Landlock baseline is too tight for a
    // legitimate workload. The broker still enforces every mediated operation;
    // only the kernel backstop against *direct* syscalls is dropped.
    if std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_some() {
        return;
    }
    let spec = spec.clone();
    // SAFETY: the closure runs in the forked child before `execve`; it only
    // issues Landlock syscalls (plus small allocations) to irrevocably drop the
    // child's ambient filesystem rights. It touches no shared parent state.
    unsafe {
        command.pre_exec(move || crate::landlock_sandbox::restrict(&spec));
    }
}

/// No-op OS-sandbox install for targets without an integration yet.
#[cfg(not(target_os = "linux"))]
pub fn install_os_sandbox(
    _command: &mut std::process::Command,
    _spec: &crate::OsSandboxSpec,
) {
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;

    use omni_capabilities::{CapabilityDomain, RequiredCapabilities};

    use crate::lower::{FsScope, classify_fs_glob, split_host_port};
    use crate::{BackendPlan, Gap, OsSandboxSpec, PatternResolver};

    /// Lower the policy's filesystem allow-subtrees into an [`OsSandboxSpec`],
    /// reporting a [`Gap`] for every pattern Landlock's allow-list-of-hierarchies
    /// model cannot express (mid-path globs, whole-fs patterns, and any `deny`).
    pub(super) fn plan(
        name: &'static str,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
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

        // Lower the `net` policy to a port-only *connect* floor. Only concrete
        // outbound ports can be a Landlock allow-list: a `host:port` rule
        // contributes `port` (any host), while an all-ports (`host:*`), missing,
        // or non-numeric port cannot be floored (it would be allow-all) and a
        // `deny` is not expressible in an allow-list. None of these are reported
        // as gaps — the OS sandbox never *claims* to cover `net` (host-level
        // enforcement stays with the shim), so there is nothing to fail closed
        // on here, exactly as with `process`.
        collect_connect_ports(req, &mut spec.connect_ports);

        // A confined child inherits the sandbox across `execve`, so any program
        // the policy allows it to spawn must have its binary readable/executable
        // under the ruleset. Record the literally-named allowed programs; the
        // spawner resolves each against `PATH` and grants its directory. Globbed
        // program patterns cannot be resolved to a path here and are left to the
        // runtime flag / script shim to gate (this is not a coverage claim — the
        // OS sandbox never covers `process`, so no gap is reported).
        if let Some(rules) = req.domains.get(&CapabilityDomain::Process) {
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
        let Some(rules) = req.domains.get(&domain) else {
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

        // Landlock grants whole hierarchies; it has no `deny` sub-path.
        for atom in &rules.deny {
            gaps.push(Gap {
                backend: name.to_string(),
                domain,
                id: atom.id,
                pattern: atom.pattern.clone(),
                reason: "Landlock grants whole path hierarchies and cannot \
                 express a `deny` sub-path; use the in-process broker"
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
        let Some(rules) = req.domains.get(&CapabilityDomain::Net) else {
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
        assert_eq!(NativeOsSandbox.tier(), Tier::OsSandbox);
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
        let cov = NativeOsSandbox.coverage();
        assert!(!cov.covers(CapabilityDomain::Net));
        assert!(!cov.covers(CapabilityDomain::Env));
        assert!(!cov.covers(CapabilityDomain::Process));
        assert_eq!(
            cov.covers(CapabilityDomain::FsRead),
            crate::landlock_sandbox::is_supported()
        );
        assert_eq!(
            cov.covers(CapabilityDomain::FsWrite),
            crate::landlock_sandbox::is_supported()
        );
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

        let plan = NativeOsSandbox.plan(&req, &roots).expect("infallible");
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

        let plan = NativeOsSandbox.plan(&req, &roots).expect("infallible");
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

        let plan = NativeOsSandbox.plan(&req, &roots).expect("infallible");
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
            !NativeOsSandbox
                .coverage()
                .covers(omni_capabilities::CapabilityDomain::Net)
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_covers_nothing_yet() {
        assert!(NativeOsSandbox.coverage().is_empty());
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
        assert!(!NativeOsSandbox::is_implemented());
    }
}
