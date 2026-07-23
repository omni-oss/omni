//! The Linux [Landlock] kernel integration behind the
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! Landlock lets an *unprivileged* process irrevocably drop its own ambient
//! filesystem rights, and — crucially — those restrictions are **inherited
//! across `execve`**. That is exactly what we need to confine a spawned JS
//! runtime: the parent (omni) installs a ruleset in the child via a `pre_exec`
//! hook, so the runtime and everything it forks are bound by it, regardless of
//! whether the runtime itself honours any permission flags. This makes it the
//! one fs-confinement mechanism that works even for runtimes with no
//! permission model of their own (e.g. Bun).
//!
//! ## Allow-list only
//!
//! Landlock grants access to whole path *hierarchies*; anything not granted is
//! denied. It has no notion of a `deny` sub-path or a glob, so a precise
//! `deny **/.git/**` or a mid-path filter cannot be expressed here — those are
//! reported as [`Gap`](crate::Gap)s by the backend and resolved by the
//! in-process broker instead. This is fundamental to Landlock's model and is
//! *not* something a newer ABI can change.
//!
//! ## ABI level (best-effort)
//!
//! The ruleset targets Landlock [`ABI::V3`], not the original `V1`, and relies
//! on the crate's default [best-effort compatibility] so an older kernel simply
//! enforces the subset it can rather than failing the spawn. Raising the ABI
//! past `V1` buys two filesystem access rights that materially affect confined
//! generators:
//!
//! * **`Refer` (V2)** — cross-directory `rename(2)`/`link(2)`. Under `V1` the
//!   kernel *always denies* a refer operation once any ruleset is active (there
//!   is no bit to grant it), so a confined runtime doing an atomic
//!   write-temp-then-rename across two allowed directories would fail. Granting
//!   `Refer` on the writable subtrees restores that (both endpoints must still
//!   be inside a granted hierarchy, so nothing escapes the sandbox).
//! * **`Truncate` (V3)** — `truncate(2)` / `O_TRUNC`, granted on the writable
//!   subtrees so ordinary file rewriting keeps working when the right becomes
//!   separately governed.
//!
//! We deliberately stop the *filesystem* ABI at `V3` rather than `V5`: `V4`
//! adds TCP network rules and `V5` adds `IoctlDev`, which — once *handled* —
//! would deny `ioctl` on device files (e.g. a runtime's `isatty`/`TIOCGWINSZ` on
//! its controlling tty) unless every such device were granted, a real
//! regression risk for no confinement gain here.
//!
//! ## Network (V4) — a port-only *connect* floor
//!
//! Landlock `V4` can restrict TCP `connect(2)`/`bind(2)` **by port**. When the
//! running kernel supports it (`abi_version() >= 4`) and the policy names
//! concrete outbound ports ([`OsSandboxSpec::connect_ports`]), we add
//! [`AccessNet::ConnectTcp`] rules for those ports so a confined script cannot
//! connect to a port the policy never allowed — an un-bypassable backstop even
//! against raw sockets / FFI that bypass the in-process shim.
//!
//! This floor is intentionally **partial**, and the fs and net handling are
//! independent (net is added only on a V4+ kernel; fs still enforces on older
//! ones):
//!
//! * It is **port-only** — Landlock cannot match a *host*, so the `host` half of
//!   a `host:port` capability is still enforced only by the (bypassable)
//!   in-process shim. That is exactly why [`NativeOsSandbox`](crate::NativeOsSandbox)
//!   does **not** claim `net` coverage: the OS floor narrows the blast radius
//!   but does not fully confine the `net` policy, so the honest
//!   [`FloorGap`](crate::FloorGap) for `net` must remain.
//! * It is **connect-only** — `bind`/`listen` is left unrestricted so a script
//!   that opens an ephemeral-port server keeps working; the `net` domain governs
//!   *outbound* access only.
//! * `host:*` (all-ports) and `deny` rules cannot be a port allow-list and are
//!   never lowered (see [`platform`](crate::platform)).
//!
//! [best-effort compatibility]: https://docs.rs/landlock/latest/landlock/enum.CompatLevel.html
//!
//! ## Fork-safety note
//!
//! [`restrict`] runs inside the `pre_exec` closure — i.e. in the forked child
//! before `execve`. It performs syscalls and small allocations there. This
//! matches the established pattern for sandboxing child processes with
//! Landlock, and is only reached after [`is_supported`] has confirmed the
//! running kernel provides Landlock.
//!
//! [Landlock]: https://docs.kernel.org/userspace-api/landlock.html
//! [`ABI::V3`]: landlock::ABI::V3

use std::io;
use std::path::{Path, PathBuf};

use landlock::{
    ABI, Access, AccessFs, AccessNet, NetPort, Ruleset, RulesetAttr,
    RulesetCreatedAttr, path_beneath_rules,
};

use crate::OsSandboxSpec;

/// `flags` value asking `landlock_create_ruleset` to *report* the supported
/// ABI version instead of creating a ruleset.
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;

/// The highest Landlock ABI version the running kernel supports, or `0` when
/// Landlock is unavailable.
///
/// Probes with `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`,
/// which returns the highest supported ABI version (`> 0`) on a Landlock-capable
/// kernel, or `-1` (`ENOSYS` when the syscall is absent, `EOPNOTSUPP` when
/// Landlock is compiled out or disabled) otherwise. It creates nothing and
/// leaks no file descriptor.
///
/// Callers use this to gate behaviour on a specific right's availability (e.g.
/// `Refer` needs `>= 2`, `Truncate` needs `>= 3`); [`restrict`] itself relies on
/// best-effort compatibility and needs no such gate.
pub fn abi_version() -> i32 {
    // SAFETY: a pure query — null ruleset pointer, zero size, VERSION flag; the
    // kernel only reads back the ABI version and touches no memory we own.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0_usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if ret > 0 { ret as i32 } else { 0 }
}

/// Whether the running kernel supports Landlock (any ABI version).
pub fn is_supported() -> bool {
    abi_version() > 0
}

/// Irrevocably restrict the calling thread/process to `spec` plus the baseline
/// paths a runtime needs to start (system libraries, `/proc`, temp, …).
///
/// Intended to be called from a `pre_exec` hook (see
/// [`install_os_sandbox`](crate::install_os_sandbox)). Best-effort compatibility
/// is used, so on a kernel that only partially supports the requested ABI the
/// strictest subset it *can* enforce is applied rather than failing the spawn.
pub fn restrict(spec: &OsSandboxSpec) -> io::Result<()> {
    // Target V3 for the *filesystem* rights (adds `Refer` in V2 and `Truncate`
    // in V3) and let the crate's default best-effort compatibility downgrade to
    // whatever the running kernel supports. Network (V4) is handled separately
    // below, gated on the kernel's actual ABI. See the module docs for why the
    // fs ABI stops at V3.
    let abi = ABI::V3;

    let mut read_paths = baseline_read_paths();
    read_paths.extend(spec.read_paths.iter().cloned());

    // A writable subtree is implicitly readable too. The safe pseudo-devices
    // (`/dev/null`, `/dev/zero`, …) are added so a confined child can use the
    // universal sink/source — `stdio: "ignore"` opens `/dev/null` read-write,
    // and many programs (e.g. `git`) open it O_RDWR unconditionally.
    let mut write_paths = baseline_write_paths();
    write_paths.extend(spec.write_paths.iter().cloned());

    let read_only = AccessFs::from_read(abi);
    let read_write = AccessFs::from_all(abi);

    // The filesystem floor is always installed (best-effort down to whatever the
    // kernel supports). The TCP `connect` floor is added only when the kernel is
    // V4+ and the policy named concrete ports, so an older kernel still enforces
    // fs. See the module docs for why this net floor is port-only and
    // connect-only.
    let net_abi = abi_version();
    let confine_net = net_abi >= 4 && !spec.connect_ports.is_empty();

    let mut ruleset = Ruleset::default()
        .handle_access(read_write)
        .map_err(to_io)?;
    if confine_net {
        ruleset = ruleset
            .handle_access(AccessNet::ConnectTcp)
            .map_err(to_io)?;
    }

    let mut created = ruleset
        .create()
        .map_err(to_io)?
        .add_rules(path_beneath_rules(existing(&read_paths), read_only))
        .map_err(to_io)?
        .add_rules(path_beneath_rules(existing(&write_paths), read_write))
        .map_err(to_io)?;

    if confine_net {
        for &port in &spec.connect_ports {
            created = created
                .add_rule(NetPort::new(port, AccessNet::ConnectTcp))
                .map_err(to_io)?;
        }
    }

    created.restrict_self().map_err(to_io)?;

    Ok(())
}

/// System directories a dynamically-linked runtime needs to *read/execute* to
/// even start (loader, shared libraries, CA certs, `/proc`, …). Granting these
/// keeps the sandbox usable; the policy's own paths add the workspace on top.
///
/// Only paths that actually exist on the host are used (see [`existing`]), so
/// this list can be generous without breaking `path_beneath_rules`.
fn baseline_read_paths() -> Vec<PathBuf> {
    [
        "/usr", "/bin", "/sbin", "/lib", "/lib64", "/lib32", "/etc", "/opt",
        "/proc", "/sys", "/dev",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Safe pseudo-devices a confined child may read *and* write. These are the
/// universal sink/source device nodes with no persistence or side effects
/// beyond the calling process; granting them keeps ordinary programs working
/// (redirecting to `/dev/null`, reading randomness) without widening access to
/// real files. Only paths that exist are used (see [`existing`]).
fn baseline_write_paths() -> Vec<PathBuf> {
    [
        "/dev/null",
        "/dev/zero",
        "/dev/full",
        "/dev/random",
        "/dev/urandom",
        "/dev/tty",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Keep only paths that exist; `path_beneath_rules` opens each path (`O_PATH`)
/// and a missing one would fail the whole ruleset.
fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| p.exists()).cloned().collect()
}

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(format!("landlock: {e}"))
}

/// Convenience for callers that have separate read/write path lists.
#[allow(dead_code)]
pub(crate) fn restrict_paths(
    read_paths: &[&Path],
    write_paths: &[&Path],
) -> io::Result<()> {
    restrict(&OsSandboxSpec {
        read_paths: read_paths.iter().map(PathBuf::from).collect(),
        write_paths: write_paths.iter().map(PathBuf::from).collect(),
        exec_programs: Vec::new(),
        connect_ports: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_agrees_with_is_supported() {
        // Pure probes — they create no ruleset and never confine the test thread
        // (which `restrict` would do irrevocably). `abi_version` is a
        // non-negative version; `is_supported` is exactly "version > 0".
        let v = abi_version();
        assert!(v >= 0, "abi version must never be negative: {v}");
        assert_eq!(is_supported(), v > 0);
    }
}
