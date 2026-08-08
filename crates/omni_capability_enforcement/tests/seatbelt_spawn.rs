//! Live end-to-end test of the macOS Seatbelt backend: it spawns **real child
//! processes** confined by [`install_os_sandbox`] (which applies a compiled SBPL
//! profile from a `pre_exec` hook) and proves the kernel denies filesystem
//! access outside the granted subtrees while permitting it inside.
//!
//! This is the OS-sandbox analog of `landlock_spawn.rs` (Linux) and
//! `appcontainer_spawn.rs` (Windows). It is macOS-only and **skips** (does not
//! fail) when the host does not provide a usable Seatbelt facility, so it is
//! safe in any CI. Setting `OMNI_REQUIRE_OS_SANDBOX` turns the skip into a hard
//! failure, so a CI that is *supposed* to exercise the floor cannot silently
//! pass by skipping.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use omni_capability_enforcement::{
    OsSandboxSpec, install_os_sandbox, seatbelt_sandbox,
};

/// Whether the confined-spawn body should run. Returns `true` when Seatbelt is
/// available (so [`install_os_sandbox`] will actually install the `pre_exec`
/// confinement). Otherwise the default is to **skip** — unless
/// `OMNI_REQUIRE_OS_SANDBOX` is set, which makes an unavailable floor a hard
/// failure so a run that *should* prove confinement cannot pass by skipping.
fn should_run_confined() -> bool {
    if seatbelt_sandbox::is_supported() {
        return true;
    }
    assert!(
        std::env::var_os("OMNI_REQUIRE_OS_SANDBOX").is_none(),
        "OMNI_REQUIRE_OS_SANDBOX is set but this host provides no usable \
         Seatbelt facility"
    );
    eprintln!("skipping: the host does not provide Seatbelt");
    false
}

/// First existing candidate path, or `None` (test then skips).
fn first_existing(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

fn cat() -> Option<PathBuf> {
    first_existing(&["/bin/cat", "/usr/bin/cat"])
}

fn sh() -> Option<PathBuf> {
    first_existing(&["/bin/sh", "/usr/bin/sh"])
}

/// A fresh scratch directory whose path is **canonicalized**.
///
/// On macOS the system temp dir lives under `/var/folders/...`, and `/var` is a
/// symlink to `/private/var`. Seatbelt matches `(subpath …)` filters against the
/// *resolved* path, so a grant naming the `/var` form would never match and even
/// an allowed access would be denied. Resolving the real path up front (and
/// deriving every child path from it) keeps both the positive and negative
/// assertions sound. The returned [`tempfile::TempDir`] must be kept alive for
/// the directory to persist.
fn scratch_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().canonicalize().expect("canonicalize tempdir");
    (dir, real)
}

fn run(mut cmd: Command, spec: &OsSandboxSpec) -> std::process::Output {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    install_os_sandbox(&mut cmd, spec).expect("installing the sandbox");
    cmd.output().expect("spawning the confined child failed")
}

/// Grant read to the given subtree(s); baseline system paths (loader, system
/// frameworks, `/dev`, …) are added by `restrict` so the program can start.
fn spec_with(read: Vec<PathBuf>, write: Vec<PathBuf>) -> OsSandboxSpec {
    OsSandboxSpec {
        read_paths: read,
        write_paths: write,
        exec_programs: Vec::new(),
        connect_ports: Vec::new(),
        confine: false,
    }
}

#[test]
fn seatbelt_confines_reads_to_the_granted_subtree() {
    if !should_run_confined() {
        return;
    }
    let Some(cat) = cat() else {
        eprintln!("skipping: no `cat` binary found");
        return;
    };

    let (_tmp, base) = scratch_dir();
    let allowed = base.join("allowed");
    let secret = base.join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();
    let ok = allowed.join("ok.txt");
    let hidden = secret.join("hidden.txt");
    fs::write(&ok, b"hello").unwrap();
    fs::write(&hidden, b"top secret").unwrap();

    // Grant read only to the `allowed` subtree; `secret` is deliberately not
    // granted.
    let spec = spec_with(vec![allowed.clone()], vec![]);

    // (1) A read inside the granted subtree succeeds.
    let mut c = Command::new(&cat);
    c.arg(&ok);
    let out = run(c, &spec);
    assert!(
        out.status.success(),
        "an allowed read must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"hello");

    // (2) A read outside it is denied by the kernel: `cat` cannot open the file
    // and exits non-zero.
    let mut c = Command::new(&cat);
    c.arg(&hidden);
    let out = run(c, &spec);
    assert!(
        !out.status.success(),
        "a read outside the granted subtree must be denied by Seatbelt"
    );
}

#[test]
fn seatbelt_confines_writes_to_the_granted_subtree() {
    if !should_run_confined() {
        return;
    }
    let Some(sh) = sh() else {
        eprintln!("skipping: no `sh` binary found");
        return;
    };

    let (_tmp, base) = scratch_dir();
    let allowed = base.join("allowed");
    let secret = base.join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();

    // Both dirs readable (so traversal/stat works), only `allowed` writable.
    let spec =
        spec_with(vec![allowed.clone(), secret.clone()], vec![allowed.clone()]);

    let write_cmd = |target: &Path| {
        let mut c = Command::new(&sh);
        c.arg("-c").arg(format!("echo hi > '{}'", target.display()));
        c
    };

    // (1) A write inside the granted subtree succeeds and lands on disk.
    let ok_target = allowed.join("w.txt");
    let out = run(write_cmd(&ok_target), &spec);
    assert!(
        out.status.success(),
        "an allowed write must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ok_target.exists(), "the allowed write did not reach disk");

    // (2) A write outside it is denied, and nothing is created.
    let bad_target = secret.join("w.txt");
    let out = run(write_cmd(&bad_target), &spec);
    assert!(
        !out.status.success(),
        "a write outside the granted subtree must be denied by Seatbelt"
    );
    assert!(
        !bad_target.exists(),
        "the denied write must not have created a file"
    );
}

#[test]
fn is_supported_is_stable() {
    // A pure probe that neither confines the test process nor leaks: it must be
    // callable repeatedly and agree with itself.
    let a = seatbelt_sandbox::is_supported();
    let b = seatbelt_sandbox::is_supported();
    assert_eq!(a, b);
}
