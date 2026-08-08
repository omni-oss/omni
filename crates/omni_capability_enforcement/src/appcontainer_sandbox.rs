//! The Windows [AppContainer] integration behind the
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! This is the Windows analog of [`landlock_sandbox`](crate::landlock_sandbox)
//! on Linux and the (still-skeleton) [`seatbelt_sandbox`](crate::seatbelt_sandbox)
//! on macOS: it confines a spawned JS runtime to the paths the policy allows.
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
//! ## How the confinement is applied
//!
//! Unlike the Unix backends — which drop rights from a `pre_exec` hook *inside*
//! the forked child — AppContainer must be established at **spawn time**: the
//! child is created *inside* the container by attaching a [`SECURITY_CAPABILITIES`]
//! to the process via `STARTUPINFOEX` +
//! [`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`]. So the Windows path diverges
//! from the Unix `pre_exec` model: instead of configuring a [`Command`] for a
//! later `spawn`, [`spawn`] performs the launch itself (building a
//! [`ProcThreadAttributeList`](std::os::windows::process::ProcThreadAttributeList)
//! and calling
//! [`CommandExt::spawn_with_attributes`](std::os::windows::process::CommandExt::spawn_with_attributes))
//! and hands back the launched [`Child`]. The AppContainer token is inherited
//! across process creation, so the runtime and everything it spawns are bound by
//! it — exactly the property that lets it confine runtimes with no permission
//! model of their own.
//!
//! ## Default-deny, widened by ACLs
//!
//! An AppContainer process is denied the filesystem by default; it keeps only
//! the ambient read/execute that `ALL APPLICATION PACKAGES` already grants on
//! system locations (so `C:\Windows`, `Program Files`, … stay usable to *start*
//! the runtime). Every path the policy grants therefore needs an explicit ACE
//! for the container's SID — there is no ambient allow. [`spawn`] lowers
//! [`OsSandboxSpec::read_paths`] into read/execute grants, [`write_paths`] into
//! read/write/execute grants, and [`OsSandboxSpec::exec_programs`] into
//! read/execute grants on each resolved binary's directory (analogous to the
//! Linux `plan`; the deny / mid-path-glob patterns remain
//! [`Gap`](crate::Gap)s the broker resolves).
//!
//! ## Grants are revoked when the confined child exits
//!
//! Those ACEs sit on the user's real files, so leaving them behind would slowly
//! litter workspace/tool directories with the container SID. [`spawn`] therefore
//! returns a [`SandboxAclGuard`] alongside the [`Child`]: the caller keeps it
//! alive for the child's whole lifetime and drops it *after* the child has
//! exited, at which point every ACE this spawn added is stripped back off. Grants
//! are reference-counted **per process** (keyed by path) so concurrent confined
//! spawns in one omni run — nested `run-generator` → `run-javascript`, or
//! parallel generators sharing the runtime/workspace dirs — only revoke a path
//! once the last child using it is gone.
//!
//! The reference count is process-local, so two **separate, concurrent** omni
//! processes that confine children touching the *same* path can still race: the
//! one that finishes first revokes an ACE the other is still using. That fails
//! **closed** (the other child then hits a denied fs op, surfaced as an error —
//! it never gains access it should not have), and the leftover/stripped ACE only
//! ever names omni's own container SID, which nothing else on the host runs as.
//! A crashed run may likewise leave an ACE behind; the next spawn re-grants
//! idempotently and cleans it up on drop.
//!
//! ## Network is out of scope here
//!
//! AppContainer's network control is a coarse capability-SID on/off switch
//! (`internetClient` and friends) that cannot express `host:port`, so — unlike
//! Landlock V4's port-only `connect_ports` floor — there is no precise net floor
//! to lower. Host-level net enforcement stays with the shim/broker, and
//! [`NativeOsSandbox`](crate::NativeOsSandbox) does not claim `net` coverage on
//! Windows (see the [`platform`](crate::platform) module docs).
//!
//! Because the container's default is hard-deny-*all*-network — every socket,
//! and even DNS resolution, blocked at the OS layer — we nonetheless *grant* the
//! client network capabilities (`internetClient` + `internetClientServer`) at
//! spawn time. Leaving them off would silently override every `net` ALLOW the
//! broker grants (an allowed `fetch` could never leave the box); granting them
//! just returns `net` to the OS default so the broker/shim — not this coarse
//! switch — remains the sole net authority.
//!
//! [`OsSandboxSpec::write_paths`]: crate::OsSandboxSpec::write_paths
//! [`write_paths`]: crate::OsSandboxSpec::write_paths
//! [AppContainer]: https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation
//! [`SECURITY_CAPABILITIES`]: https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-security_capabilities
//! [`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute

#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::process::{CommandExt as _, ProcThreadAttributeList};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_SUCCESS, HLOCAL, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS,
    GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID,
    TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, AllocateAndInitializeSid,
    DACL_SECURITY_INFORMATION, EqualSid, FreeSid, GetAce, INHERITED_ACE,
    NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID, SECURITY_CAPABILITIES,
    SID_AND_ATTRIBUTES, SID_IDENTIFIER_AUTHORITY,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES;

use crate::OsSandboxSpec;

/// A stable name for omni's AppContainer profile. The SID is derived
/// deterministically from this name (no on-disk profile is created), so every
/// omni-confined child on a host runs under the same container SID and the ACEs
/// we grant target exactly it — never a broader principal such as
/// `ALL APPLICATION PACKAGES`.
const CONTAINER_NAME: &str = "OmniOssSandbox";

// Well-known generic access-right masks (`winnt.h`). Defined locally so the
// crate does not depend on which windows-sys module happens to re-export them.
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const GENERIC_EXECUTE: u32 = 0x2000_0000;

// Well-known AppContainer *capability* SID pieces (`winnt.h`). A capability SID
// has the form `S-1-15-3-<rid>`: the app-package authority (15), the capability
// base RID (3), then the specific capability RID. We build the two client
// network capabilities from these to widen the token's default-deny network
// back to the OS default (see `NetworkCapabilities`).
const SECURITY_APP_PACKAGE_AUTHORITY: SID_IDENTIFIER_AUTHORITY =
    SID_IDENTIFIER_AUTHORITY {
        Value: [0, 0, 0, 0, 0, 15],
    };
const SECURITY_CAPABILITY_BASE_RID: u32 = 0x0000_0003;
const SECURITY_CAPABILITY_INTERNET_CLIENT: u32 = 0x0000_0001;
// `SECURITY_CAPABILITY_INTERNET_CLIENT_SERVER` (RID `0x2`, the inbound network
// *server* capability) is intentionally not defined/used: confined generators
// need outbound egress only. See `NetworkCapabilities::client`.

// `SE_GROUP_ENABLED` (`winnt.h`): the capability SID is active in the token.
// Not re-exported by `windows-sys`, so defined locally.
const SE_GROUP_ENABLED: u32 = 0x0000_0004;

/// Whether the running Windows provides a usable AppContainer facility.
///
/// AppContainer is available on Windows 8+/Server 2012+. This probes the
/// userenv/AppContainer API directly by deriving a throw-away container SID: if
/// [`DeriveAppContainerSidFromAppContainerName`] is present and succeeds the
/// facility is usable, otherwise the backend covers nothing and fails closed
/// rather than pretending to confine.
pub fn is_supported() -> bool {
    match derive_container_sid("com.omni-oss.omni.sandbox.probe") {
        Ok(sid) => {
            // SAFETY: `sid` is a valid SID returned by the derive call above.
            unsafe { FreeSid(sid) };
            true
        }
        Err(_) => false,
    }
}

/// Spawn `command` as a child launched *inside* an AppContainer confined to
/// `spec`, first widening that container's default-deny filesystem access with
/// explicit ACEs for the paths the policy grants.
///
/// Unlike the Unix backends this is applied at spawn time — the child is created
/// inside the container by attaching a [`SECURITY_CAPABILITIES`] via
/// `STARTUPINFOEX` (see the module docs) — so the module hands back the launched
/// [`Child`] rather than mutating a `Command` for a later `spawn`. The
/// container is inherited across process creation, binding the runtime and
/// everything it spawns. The child's configured stdio (e.g. piped stdin/stdout)
/// is honoured, so the caller can still drive it over pipes.
///
/// The returned [`SandboxAclGuard`] owns the cleanup of the ACEs this spawn
/// added: the caller must keep it alive for the child's whole lifetime and drop
/// it *after* the child has exited (see the module docs). Dropping it before the
/// child exits would strip grants the still-running child depends on.
///
/// Returns an error (having spawned nothing) if the container SID cannot be
/// derived or an ACE cannot be granted, so confinement failures fail closed
/// rather than launching an unconfined child.
pub fn spawn(
    command: &mut Command,
    spec: &OsSandboxSpec,
) -> io::Result<(Child, SandboxAclGuard)> {
    // Create (or, if it already exists, derive) omni's AppContainer profile and
    // reuse its SID for every ACE and for the process's SECURITY_CAPABILITIES.
    // It only needs to live through the spawn (the child's token is
    // materialised there, and the guard re-derives it at cleanup time), so it is
    // freed on the way out.
    let sid = create_or_derive_container_sid(CONTAINER_NAME)?;
    let result = spawn_with_sid(command, spec, sid);
    // SAFETY: `sid` is a valid SID from `derive_container_sid`, no longer
    // referenced once `CreateProcess` has returned (success or failure).
    unsafe { FreeSid(sid) };
    result
}

fn spawn_with_sid(
    command: &mut Command,
    spec: &OsSandboxSpec,
    sid: PSID,
) -> io::Result<(Child, SandboxAclGuard)> {
    let read = GENERIC_READ | GENERIC_EXECUTE;
    let write = GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE;

    // Build the cleanup guard as we grant. If a later grant errors and we return
    // early, dropping this partially-populated guard rolls back the ACEs already
    // added on this spawn, so a failed confined launch leaves no residue.
    let mut guard = SandboxAclGuard { paths: Vec::new() };

    for path in &spec.read_paths {
        register_grant(&mut guard, path, sid, read)?;
    }
    for path in &spec.write_paths {
        register_grant(&mut guard, path, sid, write)?;
    }
    // A confined child inherits the container across process creation, so any
    // program the policy lets it spawn must have its binary directory readable
    // and executable under the container. Resolve each literally-named program
    // to its directory (bare names are looked up on `PATH`); an unresolvable
    // name is skipped rather than failed — the OS sandbox never *claims* to
    // cover `process`, so a missing grant just leaves that gate to the runtime
    // flag / script shim.
    for prog in &spec.exec_programs {
        for dir in program_dirs(prog) {
            register_grant(&mut guard, &dir, sid, read)?;
        }
    }

    // Grant the client network capabilities so the low-privilege token does not
    // hard-block *all* network. An AppContainer with no network capability SID
    // denies every socket — and even DNS resolution — at the OS layer, which
    // would silently override every `net` ALLOW the broker/shim grants (an
    // allowed `fetch` could never leave the box). AppContainer's net control is
    // a coarse capability on/off switch that cannot express host:port, so this
    // tier never *claims* `net` coverage (see the module docs): we leave the
    // switch on and let the broker/shim remain the authoritative net enforcer.
    let net_caps = NetworkCapabilities::client()?;

    let caps = SECURITY_CAPABILITIES {
        AppContainerSid: sid,
        // Filesystem access is granted directly to the container SID via the
        // object ACLs above; the capability SIDs here only re-open `net` to the
        // OS default so the coarse switch does not override the broker/shim.
        Capabilities: net_caps.as_ptr(),
        CapabilityCount: net_caps.len(),
        Reserved: 0,
    };

    // Build the process-creation attribute list carrying the container. `caps`
    // (and the `sid` it points at) outlives `attributes` and the spawn below,
    // as `attribute` requires.
    let attributes = ProcThreadAttributeList::build()
        .attribute(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize, &caps)
        .finish()?;

    let child = command.spawn_with_attributes(&attributes)?;
    Ok((child, guard))
}

/// Grant read/execute access to `paths` for omni's AppContainer, returning a
/// [`SandboxAclGuard`] whose drop revokes them.
///
/// This is the per-call scoped grant used to admit an *already-confined* child
/// to a computed import closure after the container has been established (the
/// boot set granted at [`spawn`] time is deliberately minimal). Each path is
/// reference-counted through the same [`grant_registry`] as spawn-time grants,
/// so overlapping calls that share a path grant it once and revoke it only when
/// the last guard drops. Missing paths (and protected system dirs reachable
/// only via ambient package rights) grant nothing and are not tracked.
///
/// Fails closed — granting nothing, having rolled back any partial grant — if
/// the container SID cannot be derived or an ACE cannot be added.
pub fn grant_read_scope(paths: &[PathBuf]) -> io::Result<SandboxAclGuard> {
    let mut guard = SandboxAclGuard { paths: Vec::new() };
    if paths.is_empty() {
        return Ok(guard);
    }
    let read = GENERIC_READ | GENERIC_EXECUTE;
    let sid = create_or_derive_container_sid(CONTAINER_NAME)?;
    let result = paths
        .iter()
        .try_for_each(|path| register_grant(&mut guard, path, sid, read));
    // SAFETY: `sid` from `create_or_derive_container_sid`, no longer referenced
    // once the grants above have returned (the guard re-derives its own SID at
    // cleanup time).
    unsafe { FreeSid(sid) };
    result.map(|()| guard)
}

/// Process-wide reference counts for the paths omni has granted to its container
/// SID, keyed by the exact path passed to [`grant_path`]. A path's ACE is added
/// on the first grant and revoked only when the last confined child needing it
/// has exited (its count returns to zero), so overlapping confined spawns in one
/// process never strip a grant another is still using. See the module docs for
/// the cross-process caveat.
fn grant_registry() -> &'static Mutex<HashMap<PathBuf, usize>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, usize>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Grant `path` for the container, then — if that left a revocable omni ACE on
/// it — bump the process-wide refcount and record the path on `guard` so it is
/// released when the guard drops. Paths that grant nothing (missing, or a
/// protected system dir reachable only via ambient package rights) are not
/// tracked: there is no omni-added ACE to revoke. A path is counted at most once
/// per spawn, so a dir granted for both read and exec balances to a single
/// increment/decrement pair.
fn register_grant(
    guard: &mut SandboxAclGuard,
    path: &Path,
    sid: PSID,
    mask: u32,
) -> io::Result<()> {
    if !grant_path(path, sid, mask)? {
        return Ok(());
    }
    let key = path.to_path_buf();
    if guard.paths.contains(&key) {
        return Ok(());
    }
    let mut registry = grant_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *registry.entry(key.clone()).or_insert(0) += 1;
    guard.paths.push(key);
    Ok(())
}

/// Owns the revocation of the AppContainer filesystem grants made for one
/// confined spawn. [`spawn`] returns it beside the [`Child`]; the caller must
/// hold it for the child's whole lifetime and drop it *after* the child exits
/// (see the module docs), so a running child never loses a grant it depends on.
pub struct SandboxAclGuard {
    /// Paths this spawn reference-counted in [`grant_registry`]. Each is
    /// decremented on drop; the ACE is stripped when its count reaches zero.
    paths: Vec<PathBuf>,
}

impl Drop for SandboxAclGuard {
    fn drop(&mut self) {
        if self.paths.is_empty() {
            return;
        }
        // The guard outlives the SID the spawn held, so re-derive omni's stable
        // container SID here. Best-effort throughout: a failure only leaves a
        // benign, omni-only ACE behind, and `Drop` must never panic.
        let Ok(sid) = derive_container_sid(CONTAINER_NAME) else {
            return;
        };
        let mut registry = grant_registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for path in self.paths.drain(..) {
            let hit_zero = match registry.get_mut(&path) {
                Some(count) => {
                    *count = count.saturating_sub(1);
                    *count == 0
                }
                None => false,
            };
            if hit_zero {
                registry.remove(&path);
                let _ = revoke_path(&path, sid);
            }
        }
        // SAFETY: `sid` is a valid SID from `derive_container_sid`, unreferenced
        // once every revoke above has returned.
        unsafe { FreeSid(sid) };
    }
}

/// Create omni's AppContainer profile and return its SID, falling back to
/// deriving the SID when the profile already exists (the common case after the
/// first spawn). Registering the profile — not merely deriving the SID — is what
/// lets `CreateProcess` launch a child into the container.
///
/// The returned SID must be freed with [`FreeSid`] once the spawn that consumes
/// it has returned (see [`spawn`]).
fn create_or_derive_container_sid(name: &str) -> io::Result<PSID> {
    let wide = to_wide(name);
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: FFI. `wide` is a valid null-terminated string; the display/
    // description reuse it; no capabilities are requested; `sid` receives an
    // API-allocated SID on success.
    let hr = unsafe {
        CreateAppContainerProfile(
            wide.as_ptr(),
            wide.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr >= 0 && !sid.is_null() {
        return Ok(sid);
    }
    // `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`: the profile is already
    // registered (e.g. from a previous run), so just derive its SID.
    const ALREADY_EXISTS: i32 = 0x8007_00B7u32 as i32;
    // `HRESULT_FROM_WIN32(ERROR_BAD_ENVIRONMENT)`: a concurrent creator racing
    // the same shared profile name can transiently surface this instead of
    // `ERROR_ALREADY_EXISTS`. Both mean "someone else owns/creates this
    // profile", so derive its SID rather than failing. Because the racing
    // creator may still be mid-registration when we first try to derive, retry
    // the derive a few times with a brief backoff before giving up.
    const BAD_ENVIRONMENT: i32 = 0x8007_000Au32 as i32;
    // `E_UNEXPECTED`: observed transiently when a prior run crashed and left the
    // profile half-registered. The profile is (or becomes) registered, so the
    // same retry-and-derive recovery as `BAD_ENVIRONMENT` applies — if the
    // profile truly does not exist, the derive below fails and that error
    // propagates, so this cannot mask a genuine creation failure.
    const UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
    if hr == ALREADY_EXISTS {
        return derive_container_sid(name);
    }
    if hr == BAD_ENVIRONMENT || hr == UNEXPECTED {
        const MAX_TRIES: u32 = 5;
        let mut last_err = None;
        for attempt in 0..MAX_TRIES {
            match derive_container_sid(name) {
                Ok(sid) => return Ok(sid),
                Err(err) => last_err = Some(err),
            }
            std::thread::sleep(std::time::Duration::from_millis(
                10 * u64::from(attempt + 1),
            ));
        }
        return Err(last_err.unwrap_or_else(|| {
            io::Error::other(format!(
                "CreateAppContainerProfile({name}) raced (HRESULT \
                 0x{:08x}) and the SID could not be derived",
                hr as u32
            ))
        }));
    }
    Err(io::Error::other(format!(
        "CreateAppContainerProfile({name}) failed: HRESULT 0x{:08x}",
        hr as u32
    )))
}

/// Derive the AppContainer SID for `name` without creating a profile. Used both
/// by [`is_supported`] as a pure probe and as the fallback in
/// [`create_or_derive_container_sid`] when the profile already exists.
fn derive_container_sid(name: &str) -> io::Result<PSID> {
    let wide = to_wide(name);
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: `wide` is a valid null-terminated UTF-16 string; `sid` receives an
    // API-allocated SID on success.
    let hr = unsafe {
        DeriveAppContainerSidFromAppContainerName(wide.as_ptr(), &mut sid)
    };
    if hr < 0 || sid.is_null() {
        return Err(io::Error::other(format!(
            "DeriveAppContainerSidFromAppContainerName({name}) failed: \
             HRESULT 0x{:08x}",
            hr as u32
        )));
    }
    Ok(sid)
}

/// The string form (SDDL `S-1-15-…`) of omni's AppContainer profile SID.
///
/// Filesystem grants name the container SID through the ACL APIs directly, but a
/// pipe/object security descriptor built from an SDDL string needs the SID as
/// text. This exposes exactly that principal — the same SID `spawn` confines a
/// child under — so a DACL ACE can grant precisely the confined child access to
/// a broker channel, without duplicating the profile-name/derive logic.
pub fn container_sid_string() -> io::Result<String> {
    let sid = create_or_derive_container_sid(CONTAINER_NAME)?;
    let result = sid_to_string(sid);
    // SAFETY: `sid` from `create_or_derive_container_sid`, freed once here after
    // the string form has been copied out.
    unsafe { FreeSid(sid) };
    result
}

/// Convert a `PSID` to its SDDL string form via `ConvertSidToStringSidW`.
fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: FFI. `sid` is a valid SID; `raw` receives a `LocalAlloc`'d
    // null-terminated wide string on success, freed below.
    let ok = unsafe { ConvertSidToStringSidW(sid, &mut raw) };
    if ok == 0 || raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a valid null-terminated wide string from the call above.
    let len = unsafe { (0..).take_while(|&i| *raw.add(i) != 0).count() };
    // SAFETY: `raw` points at `len` valid `u16`s followed by the NUL.
    let slice = unsafe { std::slice::from_raw_parts(raw, len) };
    let s = String::from_utf16_lossy(slice);
    free_local(raw as HLOCAL);
    Ok(s)
}

/// Owns the client network capability SIDs attached to a confined child's
/// [`SECURITY_CAPABILITIES`] and frees them on drop. The `SID_AND_ATTRIBUTES`
/// array points *into* the owned SIDs, so an instance must outlive the spawn
/// that reads it (`spawn_with_sid` keeps it on the stack across the launch).
struct NetworkCapabilities {
    /// The capability SIDs, freed with [`FreeSid`] on drop.
    sids: Vec<PSID>,
    /// `SECURITY_CAPABILITIES.Capabilities` entries, each pointing at a `sids`
    /// element. Built after `sids` so the pointers are stable.
    attrs: Vec<SID_AND_ATTRIBUTES>,
}

impl NetworkCapabilities {
    /// `internetClient` only — enough for a runtime to make *outbound* requests
    /// to the internet and read the responses. The inbound
    /// `internetClientServer` capability (which additionally lets the container
    /// *accept* connections as a network server) is deliberately **not** granted:
    /// a generator script needs outbound egress, not the ability to listen for
    /// inbound peers, and dropping it removes an unnecessary attack surface.
    ///
    /// This does not narrow the `net` floor either way: AppContainer's network
    /// control is a coarse on/off capability that cannot express host:port, so
    /// this tier never *claims* `net` coverage — the switch is left on only so it
    /// does not silently override the broker/shim, which remains the sole
    /// authoritative `net` enforcer.
    ///
    /// Loopback to the local machine is a *separate* AppContainer exemption
    /// (admin-only, via `NetworkIsolationSetAppContainerConfig`) and is
    /// deliberately not granted here, so localhost servers remain unreachable
    /// under confinement.
    fn client() -> io::Result<Self> {
        let mut sids = Vec::with_capacity(1);
        for rid in [SECURITY_CAPABILITY_INTERNET_CLIENT] {
            sids.push(capability_sid(rid)?);
        }
        let attrs = sids
            .iter()
            .map(|&sid| SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: SE_GROUP_ENABLED,
            })
            .collect();
        Ok(Self { sids, attrs })
    }

    /// Pointer to the capability array for `SECURITY_CAPABILITIES.Capabilities`.
    fn as_ptr(&self) -> *mut SID_AND_ATTRIBUTES {
        self.attrs.as_ptr() as *mut SID_AND_ATTRIBUTES
    }

    /// Length for `SECURITY_CAPABILITIES.CapabilityCount`.
    fn len(&self) -> u32 {
        self.attrs.len() as u32
    }
}

impl Drop for NetworkCapabilities {
    fn drop(&mut self) {
        for &sid in &self.sids {
            // SAFETY: each SID came from `AllocateAndInitializeSid` and is freed
            // exactly once here.
            unsafe { FreeSid(sid) };
        }
    }
}

/// Build the well-known capability SID `S-1-15-3-<rid>` via
/// `AllocateAndInitializeSid`. The result must be freed with [`FreeSid`].
fn capability_sid(rid: u32) -> io::Result<PSID> {
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: FFI. The authority is a valid 6-byte identifier and exactly two
    // subauthorities (the capability base RID + `rid`) are supplied to match the
    // count; `sid` receives an API-allocated SID on success.
    let ok = unsafe {
        AllocateAndInitializeSid(
            &SECURITY_APP_PACKAGE_AUTHORITY,
            2,
            SECURITY_CAPABILITY_BASE_RID,
            rid,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut sid,
        )
    };
    if ok == 0 || sid.is_null() {
        return Err(io::Error::other(format!(
            "AllocateAndInitializeSid(capability {rid}) failed: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(sid)
}

/// Add a grant ACE for `sid` with the given access `mask` to `path`'s DACL,
/// preserving every existing ACE. Directories grant with subtree inheritance so
/// the whole hierarchy is covered (matching Landlock's `path_beneath` subtrees);
/// files grant without inheritance.
///
/// Returns `Ok(true)` when an omni container-SID ACE is present on the object
/// afterwards (freshly added, or already there from a prior spawn) — i.e. there
/// is something [`revoke_path`] should eventually strip. Returns `Ok(false)`
/// when nothing was granted: a path that does not exist is skipped (mirroring
/// the Linux backend's `existing` filter — a missing baseline path is nothing to
/// widen, not a failure), as is a protected system location whose descriptor a
/// normal user cannot touch (the container already reaches it via
/// `ALL APPLICATION PACKAGES`).
fn grant_path(path: &Path, sid: PSID, mask: u32) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let inheritance = if path.is_dir() {
        SUB_CONTAINERS_AND_OBJECTS_INHERIT
    } else {
        NO_INHERITANCE
    };
    let mut wide = to_wide_path(path);

    let mut old_dacl: *mut ACL = std::ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: FFI. `wide` is a valid null-terminated path; the out-params
    // receive a DACL pointer (into the descriptor) and the descriptor itself,
    // which we free below. `old_dacl` may be null (no DACL) — `SetEntriesInAclW`
    // handles that.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if rc != ERROR_SUCCESS {
        // A protected system location (e.g. `C:\Windows\System32`) denies a
        // normal user modifying — or even reading — its security descriptor. The
        // container already has ambient read/execute there via
        // `ALL APPLICATION PACKAGES` (see the module docs), so a grant is both
        // impossible and unnecessary: treat it as best-effort and move on rather
        // than aborting the whole confined spawn.
        if rc == ERROR_ACCESS_DENIED {
            return Ok(false);
        }
        return Err(win32_err("GetNamedSecurityInfoW", rc, path));
    }

    // The runtime install and other granted trees are static across spawns, yet
    // applying a subtree-inheritable ACE forces Windows to re-propagate it to
    // every descendant — seconds on a large tree (a Node install carries
    // thousands of files under `node_modules`). If our SID already holds the
    // desired grant on this object (from an earlier spawn), the propagation is
    // already done: skip the costly `SetNamedSecurityInfoW` re-write. This keeps
    // steady-state confined spawns close to unconfined cost. The grant is still
    // present, so report `true` — the caller stays responsible for revoking it.
    //
    // This short-circuit only helps *warm* spawns: the very first grant of a
    // large tree in a fresh profile still pays the full propagation. That cold
    // cost is bounded upstream by the runner resolving version-manager junctions
    // to the runtime's own (small) versioned dir before handing paths here, so
    // the granted install root is not an over-broad shared parent (see
    // `add_runtime_essentials`).
    if dacl_grants(old_dacl, sid, mask, inheritance) {
        free_local(security_descriptor as HLOCAL);
        return Ok(true);
    }

    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: mask,
        // Merge this grant into the existing DACL rather than replacing the
        // trustee's rights, so unrelated ACEs are preserved.
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: inheritance,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            // For `TRUSTEE_IS_SID`, `ptstrName` carries the SID pointer.
            ptstrName: sid as *mut u16,
        },
    };

    let mut new_dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: FFI. One explicit-access entry describing the grant; `old_dacl`
    // (possibly null) is merged into a freshly allocated `new_dacl` we free.
    let rc = unsafe { SetEntriesInAclW(1, &access, old_dacl, &mut new_dacl) };
    if rc != ERROR_SUCCESS {
        free_local(security_descriptor as HLOCAL);
        return Err(win32_err("SetEntriesInAclW", rc, path));
    }

    // SAFETY: FFI. Writes the merged DACL back onto the object.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new_dacl,
            std::ptr::null_mut(),
        )
    };
    free_local(new_dacl as HLOCAL);
    free_local(security_descriptor as HLOCAL);
    if rc != ERROR_SUCCESS {
        // As above: a protected system directory rejects the DACL write with
        // access-denied. The container already reaches it via
        // `ALL APPLICATION PACKAGES`, so skip rather than fail the spawn.
        if rc == ERROR_ACCESS_DENIED {
            return Ok(false);
        }
        return Err(win32_err("SetNamedSecurityInfoW", rc, path));
    }

    // `access` (and the `sid` its trustee points at) stays in scope through both
    // FFI calls above; nothing frees it here.
    Ok(true)
}

/// Strip omni's container-SID grant from `path`'s DACL, leaving every other ACE
/// intact — the inverse of [`grant_path`], invoked by [`SandboxAclGuard`] once no
/// confined child needs the grant. `REVOKE_ACCESS` removes *all* of the trustee's
/// ACEs on the object regardless of mask or inheritance, so one call clears both
/// a read and a write grant on the same path, and revoking an already-absent
/// grant is a harmless no-op. A missing path, or a protected system directory
/// whose descriptor a normal user cannot rewrite, is skipped.
fn revoke_path(path: &Path, sid: PSID) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut wide = to_wide_path(path);

    let mut old_dacl: *mut ACL = std::ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: FFI. `wide` is a valid null-terminated path; the out-params
    // receive a DACL pointer (into the descriptor) and the descriptor itself,
    // which we free below.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if rc != ERROR_SUCCESS {
        if rc == ERROR_ACCESS_DENIED {
            return Ok(());
        }
        return Err(win32_err("GetNamedSecurityInfoW", rc, path));
    }

    let access = EXPLICIT_ACCESS_W {
        // `REVOKE_ACCESS` ignores the permission/inheritance fields and removes
        // every ACE naming the trustee.
        grfAccessPermissions: 0,
        grfAccessMode: REVOKE_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            ptstrName: sid as *mut u16,
        },
    };

    let mut new_dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: FFI. One explicit-access entry describing the revoke; `old_dacl`
    // (possibly null) is copied into a freshly allocated `new_dacl` we free.
    let rc = unsafe { SetEntriesInAclW(1, &access, old_dacl, &mut new_dacl) };
    if rc != ERROR_SUCCESS {
        free_local(security_descriptor as HLOCAL);
        return Err(win32_err("SetEntriesInAclW", rc, path));
    }

    // SAFETY: FFI. Writes the revoked DACL back onto the object.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new_dacl,
            std::ptr::null_mut(),
        )
    };
    free_local(new_dacl as HLOCAL);
    free_local(security_descriptor as HLOCAL);
    if rc != ERROR_SUCCESS {
        if rc == ERROR_ACCESS_DENIED {
            return Ok(());
        }
        return Err(win32_err("SetNamedSecurityInfoW", rc, path));
    }
    Ok(())
}

/// Whether `dacl` already contains an allow ACE for `sid` that grants at least
/// `mask` with inheritance flags matching `inheritance` (the low
/// container/object-inherit bits). Used to make [`grant_path`] idempotent: a
/// grant left by a previous spawn need not be re-applied, avoiding a fresh
/// (and expensive) inheritable-ACE propagation across a large static tree.
///
/// A null `dacl` (object has no DACL) grants nothing, so returns `false`.
fn dacl_grants(
    dacl: *const ACL,
    sid: PSID,
    mask: u32,
    inheritance: u32,
) -> bool {
    if dacl.is_null() {
        return false;
    }
    // Only the container/object-inherit bits are meaningful for the match.
    let want_inherit = (inheritance & SUB_CONTAINERS_AND_OBJECTS_INHERIT) as u8;
    // SAFETY: `dacl` is a valid DACL pointer from `GetNamedSecurityInfoW`.
    let count = unsafe { (*dacl).AceCount };
    for i in 0..count {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `i` is in range `0..AceCount`; `ace` receives a pointer into
        // the DACL on success (non-zero return).
        if unsafe { GetAce(dacl, i as u32, &mut ace) } == 0 || ace.is_null() {
            continue;
        }
        let allowed = ace as *const ACCESS_ALLOWED_ACE;
        // SAFETY: `allowed` points at a valid ACE within the DACL.
        let header = unsafe { (*allowed).Header };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 {
            continue;
        }
        // Only explicit ACEs count: grant/revoke add and strip an ACE *on this
        // object*, never an inherited one. Counting an ACE inherited from a
        // parent (e.g. an ambient omni grant on `%TEMP%`) would make the
        // idempotency check skip adding our own revocable ACE, and would make
        // the cleanup tests observe a phantom grant that revoke can never strip.
        if header.AceFlags & (INHERITED_ACE as u8) != 0 {
            continue;
        }
        if header.AceFlags & (SUB_CONTAINERS_AND_OBJECTS_INHERIT as u8)
            != want_inherit
        {
            continue;
        }
        // SAFETY: for an allow ACE the mask precedes the trustee SID, which begins
        // at `SidStart`; its address is a valid `PSID` for `EqualSid`.
        let ace_mask = unsafe { (*allowed).Mask };
        if ace_mask & mask != mask {
            continue;
        }
        let ace_sid = unsafe { &(*allowed).SidStart as *const u32 as PSID };
        // SAFETY: both SIDs are valid; `EqualSid` returns non-zero when equal.
        if unsafe { EqualSid(ace_sid, sid) } != 0 {
            return true;
        }
    }
    false
}

/// The directories to grant read/execute so a confined child may launch `prog`
/// (and so the binary can load DLLs sitting beside it).
///
/// Resolution is deliberately *generous* on Windows, for three reasons the Unix
/// backends never face:
///  * The runtime that performs the launch (Node/Deno) may resolve a bare name
///    to a different `PATH` entry than the first match we would pick, so every
///    matching `PATH` directory is granted rather than just one.
///  * Package managers junction their install trees (scoop's `current` →
///    `<version>`), so each match is **canonicalized** — mirroring the
///    runtime-binary fix in the runner — to grant the real versioned directory
///    the loader actually reads, not the junction.
///  * A scoop-style `<name>.shim` sidecar is a *launcher* that re-execs a target
///    recorded as `path = "<target>"`. Granting only the shim's own directory
///    lets the shim start but denies the real binary it then spawns from another
///    directory (the `os error 5` seen with scoop's `git`), so the shim target's
///    directory is granted too.
///
/// An unresolvable name yields an empty list and is skipped: the OS sandbox
/// never *claims* to cover `process`, so a missing grant just leaves that gate
/// to the runtime flag / script shim (exactly as globbed patterns are).
fn program_dirs(prog: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let as_path = Path::new(prog);
    if as_path.is_absolute() || as_path.components().count() > 1 {
        push_program_dirs(as_path, &mut dirs);
        return dirs;
    }

    let Some(path_var) = std::env::var_os("PATH") else {
        return dirs;
    };
    let exts = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
    for dir in std::env::split_paths(&path_var) {
        // The name may already carry its extension.
        let direct = dir.join(prog);
        if direct.is_file() {
            push_program_dirs(&direct, &mut dirs);
        }
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let cand = dir.join(format!("{prog}{ext}"));
            if cand.is_file() {
                push_program_dirs(&cand, &mut dirs);
            }
        }
    }
    dirs
}

/// Add the (canonicalized) parent directory of the resolved binary `file`, plus
/// the target directory of a co-located scoop `.shim` sidecar if present, to
/// `dirs` (de-duplicated). See [`program_dirs`] for the rationale.
fn push_program_dirs(file: &Path, dirs: &mut Vec<PathBuf>) {
    let resolved =
        std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if let Some(parent) = resolved.parent() {
        let parent = parent.to_path_buf();
        if !dirs.contains(&parent) {
            dirs.push(parent);
        }
    }
    if let Some(target_dir) = shim_target_dir(file)
        && !dirs.contains(&target_dir)
    {
        dirs.push(target_dir);
    }
}

/// If `file` has a co-located scoop shim (`<stem>.shim`, a small TOML-ish file
/// whose `path = "<target>"` names the real binary the shim launches), return
/// that target's canonicalized parent directory. `None` when there is no shim
/// sidecar or it cannot be parsed.
fn shim_target_dir(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file.with_extension("shim")).ok()?;
    let target = text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("path")?.trim_start();
        let value = rest.strip_prefix('=')?.trim().trim_matches('"');
        (!value.is_empty()).then(|| value.to_string())
    })?;
    let target = Path::new(&target);
    let resolved =
        std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    resolved.parent().map(Path::to_path_buf)
}

/// Encode a string as a null-terminated UTF-16 buffer for the `*W` APIs.
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Encode a path as a null-terminated UTF-16 buffer for the `*W` APIs.
fn to_wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Free a `LocalAlloc`'d buffer if non-null (the descriptor from
/// `GetNamedSecurityInfoW` and the ACL from `SetEntriesInAclW`).
fn free_local(handle: HLOCAL) {
    if !handle.is_null() {
        // SAFETY: `handle` is a `LocalAlloc`'d buffer we own, freed exactly once.
        unsafe { LocalFree(handle) };
    }
}

/// Build an [`io::Error`] from a Win32 `api` call that returned `code` for
/// `path`, preserving the OS error code so callers can inspect it.
fn win32_err(api: &str, code: u32, path: &Path) -> io::Error {
    io::Error::new(
        io::Error::from_raw_os_error(code as i32).kind(),
        format!(
            "appcontainer: {api} failed for {} (error {code})",
            path.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, unique scratch directory in the system temp dir. These tests do
    /// no confinement (they only exercise pure path resolution), so the temp
    /// location carries none of the ACL-propagation cost the live spawns avoid.
    fn unique_tmp() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "omni-appcontainer-progdirs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    /// A fresh scratch directory rooted under the test binary itself (inside the
    /// repo `target/`), **not** the system temp dir. A machine that has run a
    /// confined generator holds an *inheritable* omni grant on `%TEMP%`, so a
    /// temp-based dir would inherit that ACE and defeat the ACL assertions (and
    /// an inherited ACE cannot be revoked at the child). `target/` is never
    /// granted, keeping the grant/revoke observations sound.
    fn ungranted_scratch() -> PathBuf {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "omni-appcontainer-acl-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn program_dirs_grants_the_canonicalized_parent_of_an_absolute_program() {
        let dir = unique_tmp();
        let bin = dir.join("tool.exe");
        std::fs::write(&bin, b"stub").unwrap();

        let dirs = program_dirs(&bin.to_string_lossy());
        let want = std::fs::canonicalize(&dir).unwrap();
        assert!(dirs.contains(&want), "expected {want:?} in {dirs:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn program_dirs_follows_a_scoop_shim_to_its_real_target_directory() {
        // A scoop shim is a launcher exe plus a `<stem>.shim` sidecar naming the
        // real binary it re-execs, which lives in a *different* directory. Both
        // directories must be granted (the launcher runs, then spawns the real
        // binary), so `program_dirs` must return both.
        let dir = unique_tmp();
        let shims = dir.join("shims");
        let real = dir.join("apps").join("tool").join("bin");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        let launcher = shims.join("tool.exe");
        std::fs::write(&launcher, b"launcher").unwrap();
        let target = real.join("tool.exe");
        std::fs::write(&target, b"real").unwrap();
        std::fs::write(
            shims.join("tool.shim"),
            format!("path = \"{}\"\r\n", target.display()),
        )
        .unwrap();

        let dirs = program_dirs(&launcher.to_string_lossy());
        assert!(
            dirs.contains(&std::fs::canonicalize(&shims).unwrap()),
            "launcher directory missing: {dirs:?}"
        );
        assert!(
            dirs.contains(&std::fs::canonicalize(&real).unwrap()),
            "shim target directory missing: {dirs:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shim_target_dir_returns_none_without_a_sidecar() {
        let dir = unique_tmp();
        let launcher = dir.join("bare.exe");
        std::fs::write(&launcher, b"stub").unwrap();

        assert!(shim_target_dir(&launcher).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Whether omni's container-SID grant (`mask`, directory inheritance) is
    /// currently on `path`'s DACL — the observable the cleanup tests assert on.
    fn container_ace_present(path: &Path, sid: PSID, mask: u32) -> bool {
        let wide = to_wide_path(path);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: FFI. `wide` is a valid null-terminated path; the out-params
        // receive the DACL pointer and its owning descriptor, freed below.
        let rc = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut sd,
            )
        };
        assert_eq!(rc, ERROR_SUCCESS, "reading DACL of {path:?} failed: {rc}");
        let present =
            dacl_grants(dacl, sid, mask, SUB_CONTAINERS_AND_OBJECTS_INHERIT);
        free_local(sd as HLOCAL);
        present
    }

    #[test]
    fn grant_then_revoke_round_trips_the_container_ace() {
        let dir = ungranted_scratch();
        // Skip (do not fail) where the host lacks a usable AppContainer facility,
        // matching the live spawn tests, so this stays green on any CI.
        let Ok(sid) = derive_container_sid(CONTAINER_NAME) else {
            eprintln!("skipping: the host does not provide AppContainer");
            std::fs::remove_dir_all(&dir).ok();
            return;
        };
        let read = GENERIC_READ | GENERIC_EXECUTE;

        assert!(
            !container_ace_present(&dir, sid, read),
            "a fresh directory must not already carry omni's grant"
        );
        assert!(
            grant_path(&dir, sid, read).unwrap(),
            "granting an existing directory must report a live ACE"
        );
        assert!(
            container_ace_present(&dir, sid, read),
            "the grant did not land on the DACL"
        );

        revoke_path(&dir, sid).unwrap();
        assert!(
            !container_ace_present(&dir, sid, read),
            "revoke did not strip omni's ACE back off"
        );

        // SAFETY: `sid` is a valid SID from `derive_container_sid`.
        unsafe { FreeSid(sid) };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refcounted_grant_is_revoked_only_after_the_last_guard_drops() {
        let dir = ungranted_scratch();
        let Ok(sid) = derive_container_sid(CONTAINER_NAME) else {
            eprintln!("skipping: the host does not provide AppContainer");
            std::fs::remove_dir_all(&dir).ok();
            return;
        };
        let read = GENERIC_READ | GENERIC_EXECUTE;

        // Two overlapping spawns grant the same directory (e.g. nested/parallel
        // generators sharing a runtime or workspace dir).
        let mut first = SandboxAclGuard { paths: Vec::new() };
        let mut second = SandboxAclGuard { paths: Vec::new() };
        register_grant(&mut first, &dir, sid, read).unwrap();
        register_grant(&mut second, &dir, sid, read).unwrap();
        assert!(
            container_ace_present(&dir, sid, read),
            "the shared path must be granted while in use"
        );

        // The first holder finishing must NOT strip a grant the second still
        // depends on — this is the property that keeps confined siblings sound.
        drop(first);
        assert!(
            container_ace_present(&dir, sid, read),
            "grant revoked while another confined child still needed it"
        );

        // The last holder dropping returns the refcount to zero and revokes.
        drop(second);
        assert!(
            !container_ace_present(&dir, sid, read),
            "grant outlived its last user"
        );

        // SAFETY: `sid` is a valid SID from `derive_container_sid`.
        unsafe { FreeSid(sid) };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn grant_read_scope_refcounts_and_is_a_noop_for_empty_or_missing() {
        // The per-call scoped grant shares the process-wide refcount with
        // spawn-time grants, so two overlapping scopes on the same path grant
        // its ACE once and revoke it only when the last scope drops. Empty and
        // missing paths grant nothing and are not tracked.
        let Ok(sid) = derive_container_sid(CONTAINER_NAME) else {
            eprintln!("skipping: the host does not provide AppContainer");
            return;
        };
        let read = GENERIC_READ | GENERIC_EXECUTE;

        // An empty scope is a pure no-op.
        let empty = grant_read_scope(&[]).unwrap();
        assert!(empty.paths.is_empty(), "an empty scope tracks nothing");
        drop(empty);

        // A missing path grants nothing and is not tracked.
        let missing =
            std::env::temp_dir().join("omni-oss-nonexistent-read-scope-xyz");
        std::fs::remove_dir_all(&missing).ok();
        let g = grant_read_scope(std::slice::from_ref(&missing)).unwrap();
        assert!(g.paths.is_empty(), "a missing path is not tracked");
        drop(g);

        // Overlapping scopes on the same real dir grant its ACE once.
        let dir = ungranted_scratch();
        assert!(
            !container_ace_present(&dir, sid, read),
            "a fresh scratch dir must not already carry omni's grant"
        );

        let first = grant_read_scope(std::slice::from_ref(&dir)).unwrap();
        let second = grant_read_scope(std::slice::from_ref(&dir)).unwrap();
        assert!(
            container_ace_present(&dir, sid, read),
            "the shared path must be granted while a scope holds it"
        );

        drop(first);
        assert!(
            container_ace_present(&dir, sid, read),
            "a scope dropping must not strip a grant another still holds"
        );

        drop(second);
        assert!(
            !container_ace_present(&dir, sid, read),
            "the grant must be revoked once the last scope drops"
        );

        // SAFETY: `sid` is a valid SID from `derive_container_sid`.
        unsafe { FreeSid(sid) };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_real_confined_spawn_revokes_its_grant_after_the_child_exits() {
        // Ties the revocation machinery to an actual confined launch (not just a
        // hand-rolled guard): grant a scratch dir, spawn a real child inside the
        // container, and prove the omni ACE is present while the guard is held
        // and stripped once it drops after the child exits — the property the
        // runner relies on to keep from littering user dirs with the container
        // SID.
        if !is_supported() {
            eprintln!("skipping: the host does not provide AppContainer");
            return;
        }
        let system_root = std::env::var("SystemRoot")
            .unwrap_or_else(|_| r"C:\Windows".to_string());
        let cmd = PathBuf::from(format!(r"{system_root}\System32\cmd.exe"));
        if !cmd.exists() {
            eprintln!("skipping: no cmd.exe found");
            return;
        }

        let dir = ungranted_scratch();
        let write = GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE;
        let Ok(sid) = derive_container_sid(CONTAINER_NAME) else {
            eprintln!("skipping: the host does not provide AppContainer");
            std::fs::remove_dir_all(&dir).ok();
            return;
        };
        assert!(
            !container_ace_present(&dir, sid, write),
            "a fresh scratch dir must not already carry omni's grant"
        );

        let spec = OsSandboxSpec {
            read_paths: vec![dir.clone()],
            write_paths: vec![dir.clone()],
            exec_programs: Vec::new(),
            connect_ports: Vec::new(),
            confine: false,
        };
        let mut command = Command::new(&cmd);
        command
            .current_dir(&dir)
            .arg("/c")
            .arg("exit")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (mut child, guard) =
            spawn(&mut command, &spec).expect("confined spawn failed");
        child.wait().expect("waiting on the confined child failed");

        // The grant must still be on disk while the guard lives (a running/
        // just-exited child must never lose a grant early).
        assert!(
            container_ace_present(&dir, sid, write),
            "the confined spawn did not grant the scratch dir"
        );

        // Dropping the guard after the child has exited strips the ACE back off.
        drop(guard);
        assert!(
            !container_ace_present(&dir, sid, write),
            "the grant outlived the confined child that needed it"
        );

        // SAFETY: `sid` is a valid SID from `derive_container_sid`.
        unsafe { FreeSid(sid) };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn client_grants_only_the_outbound_internet_capability() {
        // The AppContainer net capability set must carry exactly one SID: the
        // outbound `internetClient`. The inbound `internetClientServer`
        // capability (which would additionally let a confined generator *accept*
        // network connections as a server) must not be granted — a generator
        // needs egress, not the ability to listen for inbound peers.
        let caps = NetworkCapabilities::client()
            .expect("deriving the client network capability failed");
        assert_eq!(
            caps.len(),
            1,
            "expected only the outbound internetClient capability, got {}",
            caps.len()
        );
    }
}
