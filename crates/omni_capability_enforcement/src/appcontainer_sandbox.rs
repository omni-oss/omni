//! **SKELETON** — the Windows [AppContainer] integration behind the
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! This module is the intended seam for confining a spawned JS runtime on
//! Windows, mirroring [`landlock_sandbox`](crate::landlock_sandbox) on Linux and
//! [`seatbelt_sandbox`](crate::seatbelt_sandbox) on macOS. It is **not yet
//! implemented**: [`is_supported`] returns `false` and [`restrict`] returns an
//! error, so [`NativeOsSandbox`](crate::NativeOsSandbox) reports
//! [`Coverage::none`](crate::Coverage::none) on Windows and any restricted fs
//! domain falls to the in-process broker.
//!
//! ## AppContainer, not Job Objects
//!
//! The access-control analog of Landlock/Seatbelt on Windows is **AppContainer**:
//! a low-privilege token whose default-deny access to the filesystem, registry,
//! and network is widened only via object ACLs / capability SIDs. **Job Objects
//! are a different tool** (CPU/memory/process-count limits, kill-on-close) and
//! do not restrict *which* files a process may touch, so they do not belong in
//! this tier (see the [`platform`](crate::platform) module docs).
//!
//! ## Requirements (what a real implementation must provide)
//!
//! 1. **Inheritance across process creation.** The confinement must bind the
//!    spawned child and its descendants. AppContainer achieves this by creating
//!    the process *inside* the container via `STARTUPINFOEX` +
//!    `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` — note this is set at
//!    **spawn time**, not from a post-fork `pre_exec` hook, so the Windows path
//!    of [`install_os_sandbox`](crate::install_os_sandbox) must diverge from the
//!    Unix `pre_exec` model (it configures the `Command`/`STARTUPINFOEX` used to
//!    launch, rather than running code in the child).
//! 2. **`is_supported()`** — AppContainer is available on Windows 8+/Server
//!    2012+. Probe for the presence of the userenv/AppContainer APIs
//!    (`CreateAppContainerProfile` / `DeriveAppContainerSidFromAppContainerName`).
//! 3. **`restrict`/spawn wiring** — derive an AppContainer SID (profile), build
//!    the default-deny low-privilege token, then **widen** access by:
//!    * granting explicit ACEs on each read/write subtree in the
//!      [`OsSandboxSpec`] for the container's package SID (default-deny means
//!      every granted path needs an ACL entry — there is no ambient allow), and
//!    * adding the minimal **capability SIDs** required for the runtime to start.
//!    Lower [`OsSandboxSpec::read_paths`] / [`write_paths`] into ACE grants and
//!    [`OsSandboxSpec::exec_programs`] into read/execute grants on each binary's
//!    directory (analogous to the Linux `plan`; the deny/mid-path-glob patterns
//!    remain [`Gap`](crate::Gap)s the broker resolves).
//! 4. **Coverage** — report `{FsRead, FsWrite}` on Windows once this works,
//!    matching the cross-platform contract. Network is out of scope for this
//!    tier (the shim handles net).
//! 5. **Tests** — add `#[cfg(target_os = "windows")]` live tests analogous to
//!    `tests/landlock_spawn.rs`: an allowed read/write inside a granted subtree
//!    succeeds, one outside is denied; **skip** (do not fail) when AppContainer
//!    is unavailable so CI stays green.
//!
//! [AppContainer]: https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation

#![allow(dead_code)]

use std::io;

use crate::OsSandboxSpec;

/// Whether the running Windows provides a usable AppContainer facility.
///
/// **SKELETON:** always `false` until the integration lands, so the backend
/// covers nothing on Windows and fails closed rather than pretending to confine.
pub fn is_supported() -> bool {
    false
}

/// Lower `spec` into the AppContainer security capabilities/ACEs for the process
/// being launched.
///
/// **SKELETON:** not yet implemented — returns an error so no caller can mistake
/// the absence of confinement for success. Unlike the Unix backends this is
/// applied at spawn time (via `STARTUPINFOEX`), not from a `pre_exec` hook; see
/// the module docs.
pub fn restrict(_spec: &OsSandboxSpec) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AppContainer sandbox is not yet implemented on Windows",
    ))
}
