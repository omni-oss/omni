//! The macOS [Seatbelt] (`sandbox_init` / `sandbox-exec`) integration behind the
//! [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend.
//!
//! This module confines a spawned JS runtime on macOS, mirroring
//! [`landlock_sandbox`](crate::landlock_sandbox) on Linux:
//!
//! * **Profile generation (tested on any host):** the macOS startup
//!   `baseline_read_paths` and the deny-default `build_profile` SBPL generator,
//!   including the security-critical path validation that drops (as a gap) any
//!   resolved root containing an SBPL-structural character rather than risking
//!   profile injection. This is pure string generation with no macOS syscalls,
//!   so it is compiled and unit-tested on every platform under `cfg(test)` (see
//!   the module gate in `lib.rs`).
//! * **Apply (macOS-only, best-effort):** [`restrict`] builds the profile and
//!   hands it to `sandbox_init`, and [`is_supported`] reports the facility is
//!   present, so [`NativeOsSandbox`](crate::NativeOsSandbox) claims the
//!   `{FsRead, FsWrite}` floor on macOS. Any failure to confine fails the spawn
//!   **closed** — the `pre_exec` hook returns an error and the spawn is aborted
//!   rather than run unconfined. The apply path is exercised only on a macOS
//!   runner; the exact boot-essential allowance set in the profile preamble is
//!   best-effort and the live tests / CI are the source of truth (see
//!   requirement 7).
//!
//! ## Requirements (the contract the implementation follows)
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

// `restrict` intentionally does not read `Profile::dropped` (dropped roots fall
// to the broker, and this post-fork context cannot log usefully), and on a
// non-macOS `cfg(test)` build only the tests exercise the builder — so keep the
// module free of dead-code warnings for those deliberately-unread paths.
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
/// Requires a POSIX-absolute (leading `/`), UTF-8 path free of
/// [`SBPL_FORBIDDEN`] and of ASCII control characters (which include newline,
/// carriage return and tab). A path failing any of these is treated as a gap
/// rather than emitted as an unsafe clause.
///
/// The absoluteness test is a literal leading-`/` check rather than
/// [`Path::is_absolute`], because SBPL paths are always macOS/POSIX paths; using
/// the host's notion of "absolute" would misjudge them when this builder is
/// compiled and unit-tested on a non-Unix host (e.g. Windows, where `/ws` is not
/// `is_absolute()`).
fn is_encodable(path: &Path) -> bool {
    match path.to_str() {
        Some(s) => {
            s.starts_with('/')
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
/// default, the boot-critical read primitives, and the non-filesystem
/// allowances a dynamically-linked runtime (and the omni RPC bridge it talks to)
/// needs so that **only the filesystem allow-list** is actually confined by this
/// tier.
///
/// This is important: unlike Linux Landlock — which touches only the filesystem
/// (plus an optional port floor) and leaves everything else alone — Seatbelt's
/// `(deny default)` would also block networking, sockets, IPC, and signalling,
/// which would break the generator's transport to omni and often the runtime's
/// own startup. So the preamble re-allows those domains: this tier claims
/// **fs-only** coverage (see [`NativeOsSandbox`](crate::NativeOsSandbox)), and
/// `net`/`process` stay governed by the shim/broker, exactly as on Linux.
///
/// Three read primitives are boot-critical and are granted here rather than in
/// the per-subtree allow-list, because a dynamically-linked binary dies inside
/// the kernel/dyld **before `main`, with no diagnostic on stderr**, if they are
/// missing (the silent boot failure this backend hit before they were added):
/// * `(allow file-read-data (literal "/"))` — the loader/shell must read the
///   root directory itself to resolve path lookups; without it the kernel kills
///   the child during `exec`. It is a `literal` (the `/` node only), not a
///   subtree, so it does **not** widen the readable file set.
/// * `(allow file-read-metadata)` — `stat`/`lstat` on any path, which the loader
///   and path traversal into a granted subtree both need. Metadata is not file
///   *contents*: a confined generator still cannot read a file's bytes outside
///   its granted read subtrees (the negative live tests rely on this).
/// * `(allow file-map-executable)` — dyld maps the shared cache and shared
///   libraries with `PROT_EXEC`; under `(deny default)` that executable mapping
///   is otherwise refused and the process is killed at startup.
///
/// These mirror the always-emitted baseline of Microsoft's production MXC
/// Seatbelt backend, which floors reads the same way (a broad read-only system
/// baseline plus a readable `/`), so the loader works while policy paths stay
/// confined.
///
/// Only long-standing, widely-supported operation names are used, because an
/// *unrecognised* operation makes `sandbox_init` reject the whole profile and
/// fail the spawn closed. The exact boot-essential set is still best-effort and
/// validated on a macOS runner (requirement 7); the filesystem allow-list
/// appended below it is what this module generates and unit-tests.
const PROFILE_PREAMBLE: &str = "\
(version 1)
(deny default)
(allow process-fork)
(allow process-exec*)
(allow file-read-metadata)
(allow file-read-data (literal \"/\"))
(allow file-map-executable)
(allow signal (target self))
(allow sysctl-read)
(allow mach-lookup)
(allow ipc-posix-shm)
(allow system-socket)
(allow network-inbound)
(allow network-outbound)
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
/// This grants whole `/System` and `/Library` **read-only**, mirroring the
/// always-emitted baseline of Microsoft's production MXC Seatbelt backend. A
/// narrower grant (e.g. only `/System/Library`) does not boot a dynamically
/// linked runtime: on modern macOS the dyld shared cache lives behind cryptex
/// mounts and firmlinks under `/System` whose resolved paths vary across
/// releases, and missing even one makes the child die inside dyld before `main`
/// with no diagnostic. Granting the whole read-only system hierarchies is safe:
/// they are SIP-protected (unwritable regardless of the profile) and hold no
/// user data — the sensitive locations (`/Users`, and the per-user temp under
/// `/private/var/folders`) are **not** granted, so a confined generator still
/// cannot read a user's files. Read *contents* everywhere else stay denied;
/// only `stat` metadata is open (see the preamble), which is not file contents.
fn baseline_read_paths() -> Vec<PathBuf> {
    [
        // Command-line tools a runtime may exec.
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        // dyld, libSystem, system frameworks/libraries, and — on Big Sur+ /
        // Ventura+, where the system libraries live in a "cryptex" mount under
        // `/System` — the shared cache. Granting whole `/System` (SIP read-only)
        // covers every cache location without chasing per-release paths.
        "/usr/lib",
        "/usr/libexec",
        "/usr/share",
        "/System",
        "/Library",
        // Homebrew prefixes. A runtime installed via Homebrew (the common macOS
        // case, and how the CI runners provide node) is *dynamically* linked
        // against libraries in a sibling Cellar package — e.g. Homebrew `node`
        // loads `libuv`/`icu4c` from `/opt/homebrew/opt/<pkg>/lib`
        // (→ `/opt/homebrew/Cellar/<pkg>/…`), which is neither its own install
        // root nor a system prefix. Without the prefix granted, dyld is blocked
        // opening those dylibs and the runtime dies before `main`. Read-only,
        // package binaries — not user data — so this matches the baseline's
        // "system files readable, user data denied" philosophy. `/opt/homebrew`
        // is Apple Silicon; `/usr/local` covers Intel Homebrew (and mirrors the
        // Landlock baseline, which already grants it). `existing` drops the
        // absent one on any given host.
        "/usr/local",
        "/opt/homebrew",
        // Read-only system data and tool selection: timezone, the classic dyld
        // cache backing store, `/usr/bin` tool selection (`/usr/bin/python3` …),
        // and name-resolution / TLS config (`/etc` is a symlink to
        // `/private/etc`). Non-existent entries are filtered by `existing`.
        "/private/var/db/timezone",
        "/private/var/db/dyld",
        "/private/var/select",
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
/// The `sandbox_*` family ships on every supported macOS release (the SPI is
/// deprecated but present and callable), so on macOS this is `true`; on any
/// other target — where this module is compiled only under `cfg(test)` to cover
/// the pure profile builder — it is `false`, since there is nothing to apply.
pub fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// Irrevocably restrict the calling process to `spec` plus the baseline paths a
/// runtime needs to start, intended to be called from a `pre_exec` hook (see
/// [`install_os_sandbox`](crate::install_os_sandbox)).
///
/// Builds the deny-default profile with `build_profile` (read =
/// `baseline_read_paths` + `spec.read_paths`; write =
/// `unix_sandbox::writable_pseudo_devices` + `spec.write_paths`, both filtered
/// through `unix_sandbox::existing`) and applies it with `sandbox_init`. On any
/// non-macOS target this fails closed so no caller mistakes the absence of
/// confinement for success.
///
/// Like the Landlock path, this runs inside the forked child before `execve`
/// and performs small allocations and syscalls there; that is the established
/// pattern for `pre_exec` confinement and is only reached after [`is_supported`]
/// has confirmed the facility.
#[cfg(target_os = "macos")]
pub fn restrict(spec: &OsSandboxSpec) -> io::Result<()> {
    use crate::unix_sandbox::{existing, writable_pseudo_devices};

    let mut read_paths = baseline_read_paths();
    read_paths.extend(spec.read_paths.iter().cloned());

    // A writable subtree is implicitly readable too (see `build_profile`). The
    // safe pseudo-devices are added so a confined child can use the universal
    // sink/source, exactly as the Linux baseline does.
    let mut write_paths = writable_pseudo_devices();
    write_paths.extend(spec.write_paths.iter().cloned());

    let profile =
        build_profile(&existing(&read_paths), &existing(&write_paths));
    // `profile.dropped` holds roots that could not be safely encoded; they fall
    // to the in-process broker like any other gap. We cannot log usefully from
    // this post-fork context, so they are intentionally left to the broker.

    apply_profile(&profile.text)
}

/// Compile and apply an SBPL profile string to the calling process via the
/// (deprecated but ubiquitous) public `sandbox_init` entry point.
///
/// A `flags` of `0` tells `sandbox_init` to treat `profile` as a literal SBPL
/// string to compile and apply (rather than a named builtin profile). The
/// applied confinement is inherited across `execve`, so it binds the runtime
/// and everything it forks.
#[cfg(target_os = "macos")]
fn apply_profile(profile: &str) -> io::Result<()> {
    use std::ffi::{CStr, CString};

    // The literal-string form of `sandbox_init`; the SPI lives in libSystem, so
    // no explicit link attribute is needed.
    unsafe extern "C" {
        fn sandbox_init(
            profile: *const std::os::raw::c_char,
            flags: u64,
            errorbuf: *mut *mut std::os::raw::c_char,
        ) -> std::os::raw::c_int;
        fn sandbox_free_error(errorbuf: *mut std::os::raw::c_char);
    }

    let c_profile = CString::new(profile).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seatbelt: generated profile contained an interior NUL byte",
        )
    })?;

    let mut errbuf: *mut std::os::raw::c_char = std::ptr::null_mut();
    // SAFETY: `c_profile` is a valid NUL-terminated C string that outlives the
    // call; `&mut errbuf` is a valid out-pointer. On failure `sandbox_init`
    // allocates `errbuf`, which we free with `sandbox_free_error`.
    let rc = unsafe { sandbox_init(c_profile.as_ptr(), 0, &mut errbuf) };
    if rc == 0 {
        return Ok(());
    }
    let detail = if errbuf.is_null() {
        String::new()
    } else {
        // SAFETY: on failure `errbuf` points at a NUL-terminated message owned
        // by libsandbox; copy it out, then hand it back to be freed.
        let msg = unsafe { CStr::from_ptr(errbuf) }
            .to_string_lossy()
            .into_owned();
        unsafe { sandbox_free_error(errbuf) };
        format!(": {msg}")
    };
    Err(io::Error::other(format!(
        "seatbelt: sandbox_init failed (rc={rc}){detail}"
    )))
}

/// Non-macOS fallback so the module still compiles under `cfg(test)` on other
/// hosts (where only the pure profile builder is exercised). Applying a Seatbelt
/// profile is impossible off macOS, so this fails closed.
#[cfg(not(target_os = "macos"))]
pub fn restrict(_spec: &OsSandboxSpec) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "seatbelt sandbox is only available on macOS",
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
    fn preamble_leaves_non_filesystem_domains_open() {
        // Only the filesystem *read/write* set is confined by this tier; a
        // `(deny default)` that also blocked networking/IPC would break the
        // runtime's transport to omni, and one that blocked executable mapping
        // would kill a dynamically-linked runtime at boot. Guard that the
        // preamble's non-confined allowances survive into the built profile.
        let p = build_profile(&pb(&["/ws"]), &[]);
        for allow in [
            "(allow process-exec*)",
            "(allow file-read-metadata)",
            "(allow file-read-data (literal \"/\"))",
            "(allow file-map-executable)",
            "(allow network-outbound)",
            "(allow network-inbound)",
            "(allow system-socket)",
        ] {
            assert!(
                p.text.contains(allow),
                "preamble is missing {allow}:\n{}",
                p.text
            );
        }
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
    fn baseline_read_floor_excludes_user_data_hierarchies() {
        // The read baseline grants the SIP-protected system hierarchies whole
        // (`/System`, `/Library`) so a dynamically-linked runtime can boot, but
        // it must never grant the whole filesystem or any hierarchy that holds
        // user data — a confined generator must not be able to read a user's
        // home or the per-user temp store as file contents.
        let baseline = baseline_read_paths();
        for forbidden in ["/", "/Users", "/private/var/folders", "/tmp"] {
            assert!(
                !baseline.iter().any(|p| p == Path::new(forbidden)),
                "baseline must not grant the {forbidden} hierarchy as readable \
                 file contents"
            );
        }
        // Startup essentials (the loader, system libraries, the SIP system
        // hierarchies) must be present.
        for required in ["/usr/lib", "/System", "/Library", "/usr/bin"] {
            assert!(
                baseline.iter().any(|p| p == Path::new(required)),
                "baseline is missing a startup essential: {required}"
            );
        }
    }
}
