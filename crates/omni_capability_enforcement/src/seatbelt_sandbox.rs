//! The macOS [Seatbelt] (`sandbox_init` / `sandbox-exec`) integration behind the
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! This module is the seam for confining a spawned JS runtime on macOS,
//! mirroring [`landlock_sandbox`](crate::landlock_sandbox) on Linux. It is
//! **partially implemented**:
//!
//! * **Done (and tested on any host):** the platform-neutral core — the macOS
//!   startup `baseline_read_paths` and the deny-default `build_profile` SBPL
//!   generator, including the security-critical path validation that drops (as a
//!   gap) any resolved root containing an SBPL-structural character rather than
//!   risking profile injection. This is pure string generation with no macOS
//!   syscalls, so it is compiled and unit-tested on every platform under
//!   `cfg(test)` (see the module gate in `lib.rs`).
//! * **Deferred (needs a macOS runner):** the FFI that actually *applies* a
//!   compiled profile — [`is_supported`] still returns `false` and [`restrict`]
//!   still returns an error, so [`NativeOsSandbox`](crate::NativeOsSandbox)
//!   reports [`Coverage::none`](crate::Coverage::none) on macOS and every
//!   restricted fs domain falls to the in-process broker until the apply path
//!   lands. Failing closed here means no caller can mistake the absence of
//!   confinement for success.
//!
//! ## Requirements (what the remaining implementation must provide)
//!
//! 1. **Inheritance across `exec`.** Like Landlock, the confinement must bind
//!    the spawned child *and everything it forks*, so it can confine runtimes
//!    with no permission model of their own (Bun). Seatbelt profiles are
//!    inherited across `execve`, which satisfies this.
//! 2. **`is_supported()`** — probe that the Seatbelt facility is usable on the
//!    running OS (the `sandbox_init`/`sandbox_compile` family is present on all
//!    supported macOS versions, but the SPI is deprecated; gate on it being
//!    callable and, if using `sandbox-exec`, on that binary existing).
//! 3. **`restrict(&OsSandboxSpec)`** — build the profile with `build_profile`
//!    (read = `baseline_read_paths` + `spec.read_paths`; write =
//!    `unix_sandbox::writable_pseudo_devices` + `spec.write_paths`, both filtered
//!    through `unix_sandbox::existing` exactly as the Linux `landlock_sandbox`
//!    baseline does), then compile and apply it from a
//!    `pre_exec` hook (see [`install_os_sandbox`](crate::install_os_sandbox))
//!    exactly like the Linux path. Two viable apply mechanisms:
//!    * hand the profile's `text` to `sandbox_init`/`sandbox_compile` +
//!      `sandbox_apply` (parameterising paths via `sandbox_init_with_parameters`
//!      would let the paths be bound as params instead of interpolated, removing
//!      the escaping concern entirely — a future refinement); or
//!    * re-exec via `/usr/bin/sandbox-exec -p <profile>` (simpler, avoids the
//!      deprecated SPI, but adds a wrapper process).
//!
//!    **Escaping is security-critical**, and [`build_profile`] already handles
//!    it: SBPL is a TinyScheme dialect, so a path is a Scheme string literal and
//!    an unescaped `"` or `)` in a grant path would let a crafted root name
//!    terminate the string/list early and inject additional `(allow …)` clauses
//!    (a profile-injection fail-open). Rather than escape, the builder *rejects*
//!    any path containing `" ( ) \` or a control character and reports it in the
//!    profile's `dropped` list, so such a subtree falls to the broker as a gap —
//!    mirroring how the Deno backend rejects flag values embedding `,`/`=`.
//! 4. **Coverage** — the backend should report `{FsRead, FsWrite}` on macOS once
//!    apply lands (see [`NativeOsSandbox::coverage`](crate::NativeOsSandbox)),
//!    matching Landlock, which claims fs only. Seatbelt can *also* express
//!    `network*` rules: note the Linux backend now installs a **partial** net
//!    floor (Landlock V4's port-only, connect-only `connect_ports` — see
//!    [`landlock_sandbox`](crate::landlock_sandbox)), so net is no longer wholly
//!    out of scope for this tier. It still does **not** *claim* `net` coverage,
//!    though — host-level net enforcement stays with the shim/broker — so a
//!    Seatbelt impl may optionally mirror that partial floor but should keep its
//!    reported coverage to fs to match the cross-platform contract.
//! 5. **Coarse, allow-list only.** Like Landlock, Seatbelt grants subtrees and
//!    cannot express a precise `deny **/.git/**`; those remain [`Gap`](crate::Gap)s
//!    resolved by the broker (the `platform` module's `lowering::plan` is the
//!    reference for lowering the spec into subtrees and reporting deny/mid-path
//!    globs as gaps — it is platform-neutral and already feeds every OS backend,
//!    so a macOS impl needs no `plan` of its own).
//! 6. **`exec_programs` — nothing to do here.** Unlike the Windows AppContainer
//!    backend (which resolves `exec_programs` to binary directories at *spawn*
//!    time), a Unix pre-spawn backend receives those directories already folded
//!    into `read_paths`: the runner's `add_runtime_essentials` resolves each
//!    allowed program on `PATH` and pushes its directory into the spec's
//!    `read_paths`, then `std::mem::take`s `exec_programs` **before** the spec
//!    reaches this hook. So `spec.exec_programs` is always empty in `restrict`;
//!    confining `read_paths` (as in requirement 3) already grants the
//!    program dirs, exactly as Landlock does. Do **not** add an `exec_programs`
//!    loop here — it would be dead code.
//! 7. **Tests** — the pure profile generation is covered here on any host. The
//!    remaining apply path needs `#[cfg(target_os = "macos")]` live tests
//!    analogous to `tests/landlock_spawn.rs`: an allowed read/write inside a
//!    granted subtree succeeds, one outside is denied by the kernel; **skip**
//!    (do not fail) when the facility is unavailable, so CI without it stays
//!    green.
//!
//! [Seatbelt]: https://newosxbook.com/files/HITSB.pdf

// The apply path is still deferred, so on macOS `build_profile` /
// `baseline_read_paths` are not yet called by `restrict`; keep them without a
// dead-code warning until that lands (the unit tests exercise them on every
// host under `cfg(test)`).
#![allow(dead_code)]

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use crate::OsSandboxSpec;

/// A compiled-ready Seatbelt profile plus the roots that could not be encoded.
pub(crate) struct Profile {
    /// The SBPL source, ready for `sandbox_compile_string` / `sandbox_init`.
    pub(crate) text: String,
    /// Resolved roots omitted from `text` because they contain an
    /// SBPL-structural character (see [`is_encodable`]) and encoding them would
    /// risk profile injection. Like any other [`Gap`](crate::Gap) these fall to
    /// the in-process broker; the caller should surface them (e.g. a warning) so
    /// a path that silently lost its *kernel* floor is at least visible.
    pub(crate) dropped: Vec<PathBuf>,
}

/// Characters that must never reach an SBPL string literal. SBPL is a TinyScheme
/// dialect, so `"` closes the string and `(`/`)` are list structure; a path
/// carrying any of them (or a raw control character such as a newline) could
/// terminate the `(subpath "…")` filter early and inject further clauses. We
/// *reject* rather than escape, so the emitted literal is always unambiguous and
/// needs no escaping.
const SBPL_FORBIDDEN: &[char] = &['"', '\\', '(', ')'];

/// Whether `path` can be safely interpolated into an SBPL string literal.
///
/// Requires an absolute, UTF-8 path free of [`SBPL_FORBIDDEN`] and of ASCII
/// control characters (which include newline, carriage return and tab). A path
/// failing any of these is treated as a gap rather than emitted as an unsafe
/// clause.
fn is_encodable(path: &Path) -> bool {
    match path.to_str() {
        Some(s) => {
            path.is_absolute()
                && !s.contains(SBPL_FORBIDDEN)
                && !s.chars().any(|c| c.is_control())
        }
        // A non-UTF-8 path cannot be written into a Scheme string.
        None => false,
    }
}

/// Build a deny-default SBPL allow-list confining reads to `read_paths` and
/// writes to `write_paths`.
///
/// Each list is expected to already include the relevant baseline (the caller
/// folds in [`baseline_read_paths`] and
/// [`writable_pseudo_devices`](crate::unix_sandbox::writable_pseudo_devices) and
/// filters through [`existing`](crate::unix_sandbox::existing), mirroring
/// `landlock_sandbox::restrict`). A writable subtree is granted read access too,
/// matching Landlock's semantics (there `from_all` implies read on the writable
/// rules), so every write root also appears in the read allow-list.
///
/// Any root that is not [`is_encodable`] is dropped from the profile and
/// reported in [`Profile::dropped`]; it is never escaped into a clause.
pub(crate) fn build_profile(
    read_paths: &[PathBuf],
    write_paths: &[PathBuf],
) -> Profile {
    let mut dropped = Vec::new();

    // Writable subtrees are implicitly readable, so the read allow-list is the
    // union of read and write roots (deduplicated, order-preserving).
    let mut read_roots: Vec<&Path> = Vec::new();
    for p in read_paths.iter().chain(write_paths.iter()) {
        let p = p.as_path();
        if !is_encodable(p) {
            // Only record a drop once (dedup keeps the reported set clean).
            if !dropped.iter().any(|d: &PathBuf| d.as_path() == p) {
                dropped.push(p.to_path_buf());
            }
            continue;
        }
        if !read_roots.contains(&p) {
            read_roots.push(p);
        }
    }

    let mut write_roots: Vec<&Path> = Vec::new();
    for p in write_paths {
        let p = p.as_path();
        if !is_encodable(p) {
            if !dropped.iter().any(|d: &PathBuf| d.as_path() == p) {
                dropped.push(p.to_path_buf());
            }
            continue;
        }
        if !write_roots.contains(&p) {
            write_roots.push(p);
        }
    }

    let mut text = String::new();
    text.push_str(PROFILE_PREAMBLE);
    push_allow(&mut text, "file-read*", &read_roots);
    push_allow(&mut text, "file-write*", &write_roots);

    Profile { text, dropped }
}

/// The fixed head of every generated profile: SBPL version, a deny-everything
/// default, and the minimal non-filesystem allowances a dynamically-linked
/// runtime needs to boot and `execve` the target (fork/exec, reading sysctls,
/// and Mach service lookup). The exact boot-essential set must be validated on a
/// macOS runner (requirement 7); the filesystem allow-list below it is what this
/// module generates and tests.
const PROFILE_PREAMBLE: &str = "\
(version 1)
(deny default)
(allow process-fork)
(allow process-exec*)
(allow sysctl-read)
(allow mach-lookup)
";

/// Append `(allow <operation> (subpath "…") …)` for `roots`, or nothing when
/// `roots` is empty (an empty `allow` would be a no-op clause).
fn push_allow(text: &mut String, operation: &str, roots: &[&Path]) {
    if roots.is_empty() {
        return;
    }
    let _ = write!(text, "(allow {operation}");
    for root in roots {
        // `root` is `is_encodable`, so `to_str()` is `Some` and free of `"`/`\`.
        let s = root.to_str().expect("is_encodable guarantees UTF-8");
        let _ = write!(text, " (subpath \"{s}\")");
    }
    text.push_str(")\n");
}

/// System directories a dynamically-linked macOS runtime needs to *read/execute*
/// to even start (dyld and the shared cache, the system frameworks/libraries,
/// the command-line tool dirs it may `execve`, read-only ICU/timezone data,
/// resolver/TLS config under `/private/etc`, and the coarse `/dev`). The policy's
/// own paths add the workspace on top.
///
/// Only paths that actually exist on the host should be used (the caller filters
/// through [`existing`](crate::unix_sandbox::existing)), so this list can name
/// locations that vary across macOS versions without breaking the profile.
///
/// Like the Linux baseline it is deliberately **narrowed** — no whole `/`,
/// `/System`, `/Library`, or `/Users` — to what startup genuinely needs, so a
/// confined generator cannot read arbitrary files. The exact set must be
/// validated on a macOS runner (requirement 7); on Big Sur and later the dyld
/// shared cache location in particular has moved, so `is_supported` / the live
/// tests are the source of truth for boot-completeness.
fn baseline_read_paths() -> Vec<PathBuf> {
    [
        // dyld, libSystem, and the shared cache the loader maps at startup.
        "/usr/lib",
        "/usr/libexec",
        "/System/Library",
        "/private/var/db/dyld",
        // Command-line tools a runtime may exec.
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        // Read-only system data: ICU/timezone, etc.
        "/usr/share",
        // Name resolution + TLS config (`/etc` is a symlink to `/private/etc`).
        "/private/etc",
        // Device nodes dir kept coarse on purpose (the writable pseudo-devices
        // are granted separately); it holds nodes, not file contents.
        "/dev",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Whether the running macOS provides a usable Seatbelt facility.
///
/// **Deferred:** always `false` until the apply/FFI path lands, so the backend
/// covers nothing on macOS and fails closed rather than pretending to confine.
pub fn is_supported() -> bool {
    false
}

/// Irrevocably restrict the calling process to `spec` (plus the baseline paths a
/// runtime needs to start), intended to be called from a `pre_exec` hook.
///
/// **Deferred:** the profile *generation* is implemented (see [`build_profile`]),
/// but applying it requires the macOS `sandbox_*` FFI, which is not yet wired —
/// this returns an error so no caller can mistake the absence of confinement for
/// success. See the module docs for the required behaviour.
pub fn restrict(_spec: &OsSandboxSpec) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "seatbelt sandbox apply path is not yet implemented on macOS",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn profile_is_deny_default_with_versioned_header() {
        let p = build_profile(&pb(&["/ws"]), &[]);
        assert!(p.text.starts_with("(version 1)\n"));
        assert!(p.text.contains("(deny default)"));
    }

    #[test]
    fn read_and_write_roots_become_subpath_clauses() {
        let p = build_profile(&pb(&["/ws/read"]), &pb(&["/ws/write"]));
        assert!(p.text.contains("(allow file-read*"), "profile:\n{}", p.text);
        assert!(p.text.contains("(subpath \"/ws/read\")"));
        assert!(p.text.contains("(allow file-write*"));
        assert!(p.text.contains("(subpath \"/ws/write\")"));
        assert!(p.dropped.is_empty());
    }

    #[test]
    fn writable_subtree_is_also_readable() {
        // Matches Landlock semantics: a write root must appear in the read
        // allow-list too, so the write path is granted for both operations.
        let p = build_profile(&[], &pb(&["/ws/write"]));
        let read_line = p
            .text
            .lines()
            .find(|l| l.starts_with("(allow file-read*"))
            .expect("a read clause should exist for the writable subtree");
        assert!(
            read_line.contains("(subpath \"/ws/write\")"),
            "read clause missing writable subtree: {read_line}"
        );
    }

    #[test]
    fn empty_spec_emits_no_allow_clauses() {
        let p = build_profile(&[], &[]);
        assert!(p.text.contains("(deny default)"));
        assert!(!p.text.contains("(allow file-read*"));
        assert!(!p.text.contains("(allow file-write*"));
        assert!(p.dropped.is_empty());
    }

    #[test]
    fn duplicate_roots_are_emitted_once() {
        let p = build_profile(&pb(&["/ws", "/ws"]), &[]);
        assert_eq!(
            p.text.matches("(subpath \"/ws\")").count(),
            1,
            "a repeated root must be emitted a single time:\n{}",
            p.text
        );
    }

    #[test]
    fn paths_with_sbpl_metacharacters_are_dropped_not_escaped() {
        // A crafted root name trying to close the string/list and inject an
        // extra grant must never reach the profile text.
        let evil = "/ws\") (allow file-write* (subpath \"/";
        let p = build_profile(&pb(&["/safe", evil]), &[]);
        assert!(p.text.contains("(subpath \"/safe\")"));
        assert!(
            !p.text.contains(evil),
            "injection payload leaked into the profile:\n{}",
            p.text
        );
        // The only file-write* clause must be absent (we granted no writes); an
        // injected one would have appeared via the payload above.
        assert!(!p.text.contains("(allow file-write*"));
        assert_eq!(p.dropped, pb(&[evil]));
    }

    #[test]
    fn control_characters_and_relative_paths_are_dropped() {
        let newline = "/ws\n/etc";
        let relative = "relative/path";
        let p = build_profile(&pb(&[newline, relative, "/ok"]), &[]);
        assert!(p.text.contains("(subpath \"/ok\")"));
        assert!(p.dropped.contains(&PathBuf::from(newline)));
        assert!(p.dropped.contains(&PathBuf::from(relative)));
    }

    #[test]
    fn is_encodable_accepts_ordinary_absolute_paths() {
        assert!(is_encodable(Path::new("/Users/me/My Project/src")));
        assert!(!is_encodable(Path::new("/has\"quote")));
        assert!(!is_encodable(Path::new("/has(paren")));
        assert!(!is_encodable(Path::new("/has)paren")));
        assert!(!is_encodable(Path::new("/has\\backslash")));
        assert!(!is_encodable(Path::new("relative")));
    }

    #[test]
    fn baseline_read_floor_is_narrowed_not_whole_hierarchies() {
        // Guard against regressing to granting entire top-level trees, and keep
        // the startup essentials present.
        let baseline = baseline_read_paths();
        for forbidden in ["/", "/System", "/Library", "/Users"] {
            assert!(
                !baseline.iter().any(|p| p == Path::new(forbidden)),
                "baseline must not grant the whole {forbidden} hierarchy"
            );
        }
        for required in ["/usr/lib", "/System/Library"] {
            assert!(
                baseline.iter().any(|p| p == Path::new(required)),
                "baseline is missing a startup essential: {required}"
            );
        }
    }
}
