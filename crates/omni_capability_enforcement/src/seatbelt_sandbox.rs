//! **SKELETON** — the macOS [Seatbelt] (`sandbox_init` / `sandbox-exec`)
//! integration behind the [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! This module is the intended seam for confining a spawned JS runtime on
//! macOS, mirroring [`landlock_sandbox`](crate::landlock_sandbox) on Linux. It
//! is **not yet implemented**: [`is_supported`] returns `false` and [`restrict`]
//! returns an error, so [`NativeOsSandbox`](crate::NativeOsSandbox) reports
//! [`Coverage::none`](crate::Coverage::none) on macOS and any restricted fs
//! domain falls to the in-process broker (Linux-parity fs confinement at the
//! kernel level is simply absent until this lands).
//!
//! ## Requirements (what a real implementation must provide)
//!
//! 1. **Inheritance across `exec`.** Like Landlock, the confinement must bind
//!    the spawned child *and everything it forks*, so it can confine runtimes
//!    with no permission model of their own (Bun). Seatbelt profiles are
//!    inherited across `execve`, which satisfies this.
//! 2. **`is_supported()`** — probe that the Seatbelt facility is usable on the
//!    running OS (the `sandbox_init`/`sandbox_compile` family is present on all
//!    supported macOS versions, but the SPI is deprecated; gate on it being
//!    callable and, if using `sandbox-exec`, on that binary existing).
//! 3. **`restrict(&OsSandboxSpec)`** — apply an allow-list profile derived from
//!    the spec, run from a `pre_exec` hook (see
//!    [`install_os_sandbox`](crate::install_os_sandbox)) exactly like the Linux
//!    path. Two viable mechanisms:
//!    * compile a **SBPL profile** string (deny default; `(allow file-read*
//!      (subpath "…"))` per read root; `(allow file-write* (subpath "…"))` per
//!      write root; a baseline granting the loader/`/usr`/`/System`/`/dev`
//!      pseudo-devices so the runtime can start, analogous to
//!      [`landlock_sandbox::baseline_read_paths`]) and hand it to
//!      `sandbox_init`/`sandbox_compile` + `sandbox_apply`; or
//!    * re-exec via `/usr/bin/sandbox-exec -p <profile>` (simpler, avoids the
//!      deprecated SPI, but adds a wrapper process).
//! 4. **Coverage** — the backend should report `{FsRead, FsWrite}` on macOS once
//!    this works (see [`NativeOsSandbox::coverage`](crate::NativeOsSandbox)),
//!    matching Landlock. Seatbelt can *also* express `network*` rules, but net
//!    is out of scope for the OS-sandbox tier today (the shim handles it); keep
//!    coverage to fs to match the cross-platform contract.
//! 5. **Coarse, allow-list only.** Like Landlock, Seatbelt grants subtrees and
//!    cannot express a precise `deny **/.git/**`; those remain [`Gap`]s resolved
//!    by the broker (`platform::linux::plan` is the reference for lowering the
//!    spec into subtrees and reporting deny/mid-path globs as gaps — a macOS
//!    `plan` should share that logic).
//! 6. **`exec_programs`** — grant read/execute on each allowed program's binary
//!    directory so a confined child may `execve` it, mirroring the Linux path.
//! 7. **Tests** — add `#[cfg(target_os = "macos")]` live tests analogous to
//!    `tests/landlock_spawn.rs`: an allowed read/write inside a granted subtree
//!    succeeds, one outside is denied by the kernel; **skip** (do not fail) when
//!    the facility is unavailable, so CI without it stays green.
//!
//! [Seatbelt]: https://newosxbook.com/files/HITSB.pdf

#![allow(dead_code)]

use std::io;

use crate::OsSandboxSpec;

/// Whether the running macOS provides a usable Seatbelt facility.
///
/// **SKELETON:** always `false` until the integration lands, so the backend
/// covers nothing on macOS and fails closed rather than pretending to confine.
pub fn is_supported() -> bool {
    false
}

/// Irrevocably restrict the calling process to `spec` (plus the baseline paths a
/// runtime needs to start), intended to be called from a `pre_exec` hook.
///
/// **SKELETON:** not yet implemented — returns an error so no caller can mistake
/// the absence of confinement for success. See the module docs for the required
/// behaviour.
pub fn restrict(_spec: &OsSandboxSpec) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "seatbelt sandbox is not yet implemented on macOS",
    ))
}
