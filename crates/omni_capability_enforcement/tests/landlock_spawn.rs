//! Live end-to-end test of the Linux Landlock backend: it spawns **real child
//! processes** confined by [`install_os_sandbox`] and proves the kernel denies
//! filesystem access outside the granted subtrees while permitting it inside.
//!
//! This is the OS-sandbox analog of `deno_spawn.rs` (which proves the Deno
//! pre-spawn flags). It is Linux-only and **skips** (does not fail) when the
//! running kernel does not provide Landlock, so it is safe in any CI.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use omni_capability_enforcement::{
    OsSandboxSpec, install_os_sandbox, landlock_sandbox,
};

/// First existing candidate path, or `None` (test then skips).
fn first_existing(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

fn cat() -> Option<PathBuf> {
    first_existing(&["/usr/bin/cat", "/bin/cat"])
}

fn sh() -> Option<PathBuf> {
    first_existing(&["/bin/sh", "/usr/bin/sh"])
}

fn mv() -> Option<PathBuf> {
    first_existing(&["/usr/bin/mv", "/bin/mv"])
}

fn bash() -> Option<PathBuf> {
    first_existing(&["/usr/bin/bash", "/bin/bash"])
}

fn run(mut cmd: Command, spec: &OsSandboxSpec) -> std::process::Output {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    install_os_sandbox(&mut cmd, spec);
    cmd.output().expect("spawning the confined child failed")
}

/// Like [`run`], but tolerates the spawn *itself* failing — when Landlock denies
/// `execve` of the target binary, the child never starts and the parent sees an
/// error rather than a non-zero exit. Returns `true` iff the program ran to a
/// successful exit.
/// Whether `cmd` (confined by `spec`) runs to a successful exit.
///
/// When Landlock denies `execve` the child never starts and the parent sees an
/// error rather than a non-zero exit. Freshly-written binaries can also fail to
/// exec *transiently* under heavy parallel load (e.g. `ETXTBSY`, or a not-yet
/// settled page cache), so a genuinely-executable program is retried a few
/// times: a program that is actually denied fails every attempt (returning
/// `false`), while a runnable one succeeds on some attempt.
fn ran_successfully(
    cmd_fn: impl Fn() -> Command,
    spec: &OsSandboxSpec,
) -> bool {
    for attempt in 0..5 {
        let mut cmd = cmd_fn();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        install_os_sandbox(&mut cmd, spec);
        if let Ok(out) = cmd.output()
            && out.status.success()
        {
            return true;
        }
        if attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    false
}

#[test]
fn landlock_confines_reads_to_the_granted_subtree() {
    if !landlock_sandbox::is_supported() {
        eprintln!("skipping: the running kernel does not provide Landlock");
        return;
    }
    let Some(cat) = cat() else {
        eprintln!("skipping: no `cat` binary found");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let allowed = dir.path().join("allowed");
    let secret = dir.path().join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();
    let ok = allowed.join("ok.txt");
    let hidden = secret.join("hidden.txt");
    fs::write(&ok, b"hello").unwrap();
    fs::write(&hidden, b"top secret").unwrap();

    // Grant read only to the `allowed` subtree (baseline system paths are added
    // by `restrict` so the runtime can start); `secret` is deliberately not
    // granted.
    let spec = OsSandboxSpec {
        read_paths: vec![allowed.clone()],
        write_paths: vec![],
        exec_programs: vec![],
        connect_ports: vec![],
    };

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

    // (2) A read outside it is denied by the kernel.
    let mut c = Command::new(&cat);
    c.arg(&hidden);
    let out = run(c, &spec);
    assert!(
        !out.status.success(),
        "a read outside the granted subtree must be denied by Landlock"
    );
}

#[test]
fn landlock_confines_writes_to_the_granted_subtree() {
    if !landlock_sandbox::is_supported() {
        eprintln!("skipping: the running kernel does not provide Landlock");
        return;
    }
    let Some(sh) = sh() else {
        eprintln!("skipping: no `sh` binary found");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let allowed = dir.path().join("allowed");
    let secret = dir.path().join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();

    // Both dirs are readable (so traversal/stat works), but only `allowed` is
    // writable.
    let spec = OsSandboxSpec {
        read_paths: vec![allowed.clone(), secret.clone()],
        write_paths: vec![allowed.clone()],
        exec_programs: vec![],
        connect_ports: vec![],
    };

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
        "a write outside the granted subtree must be denied by Landlock"
    );
    assert!(
        !bad_target.exists(),
        "the denied write must not have created a file"
    );
}

#[test]
fn landlock_permits_cross_directory_rename_within_a_granted_writable_subtree() {
    // Proves the ABI was raised past V1: a cross-directory `rename(2)` is a
    // Landlock `refer` operation, which V1 denies unconditionally and V2+ can
    // grant. Skips (does not fail) on a kernel whose Landlock ABI predates the
    // `refer` access right.
    if !landlock_sandbox::is_supported() {
        eprintln!("skipping: the running kernel does not provide Landlock");
        return;
    }
    if landlock_sandbox::abi_version() < 2 {
        eprintln!("skipping: kernel Landlock ABI < 2 lacks the `refer` right");
        return;
    }
    let Some(mv) = mv() else {
        eprintln!("skipping: no `mv` binary found");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let allowed = dir.path().join("allowed");
    let secret = dir.path().join("secret");
    let src_dir = allowed.join("a");
    let dst_dir = allowed.join("b");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&dst_dir).unwrap();
    fs::create_dir(&secret).unwrap();
    let src = src_dir.join("f.txt");
    fs::write(&src, b"payload").unwrap();

    // Grant read+write to the whole `allowed` subtree; `secret` is granted
    // nothing, so a rename *out* of the sandbox has no destination hierarchy.
    let spec = OsSandboxSpec {
        read_paths: vec![],
        write_paths: vec![allowed.clone()],
        exec_programs: vec![],
        connect_ports: vec![],
    };

    // (1) A cross-directory rename *within* the granted subtree (a `refer`
    // operation) succeeds now that `Refer` is granted; under V1 the kernel would
    // have denied it unconditionally.
    let dst = dst_dir.join("f.txt");
    let mut c = Command::new(&mv);
    c.arg(&src).arg(&dst);
    let out = run(c, &spec);
    assert!(
        out.status.success(),
        "a cross-directory rename inside the granted subtree must succeed \
         under ABI >= 2; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dst.exists(),
        "the renamed file must land at its destination"
    );
    assert!(
        !src.exists(),
        "the source must be gone after a successful rename"
    );

    // (2) A rename that would move the file *out* of the granted subtree is
    // denied: `refer` requires both endpoints inside a granted hierarchy, and
    // `secret` is granted nothing.
    let escaped = secret.join("f.txt");
    let mut c = Command::new(&mv);
    c.arg(&dst).arg(&escaped);
    let out = run(c, &spec);
    assert!(
        !out.status.success(),
        "a rename escaping the granted subtree must be denied by Landlock"
    );
    assert!(
        !escaped.exists(),
        "the denied rename must not have created the file"
    );
}

#[test]
fn landlock_gates_execution_by_the_binary_directory_grant() {
    // The security property behind `OsSandboxSpec::exec_programs`: a confined
    // process can only `execve` a program whose *directory* is granted. The
    // runner resolves each allowed program name to its binary directory and adds
    // it to `read_paths` (a read grant includes execute under Landlock); this
    // test proves that granting the directory is both necessary and sufficient,
    // independent of the name-resolution the runner performs.
    if !landlock_sandbox::is_supported() {
        eprintln!("skipping: the running kernel does not provide Landlock");
        return;
    }
    let Some(cat) = cat() else {
        eprintln!("skipping: no `cat` binary found");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let granted = dir.path().join("granted");
    let offlimits = dir.path().join("offlimits");
    fs::create_dir(&granted).unwrap();
    fs::create_dir(&offlimits).unwrap();

    // A copy of a real, dynamically-linked program in each directory. `fs::copy`
    // preserves the executable mode bits on Unix, so both copies are runnable in
    // the absence of a sandbox; the loader and shared libraries they need live
    // under the baseline system prefixes `restrict` always grants.
    let granted_bin = granted.join("prog");
    let offlimits_bin = offlimits.join("prog");
    fs::copy(&cat, &granted_bin).unwrap();
    fs::copy(&cat, &offlimits_bin).unwrap();

    // Grant read/execute for the `granted` directory only — exactly what the
    // runner does for an allowed program's resolved binary directory.
    let spec = OsSandboxSpec {
        read_paths: vec![granted.clone()],
        write_paths: vec![],
        exec_programs: vec![],
        connect_ports: vec![],
    };

    // (1) A program whose directory is granted can be executed. `cat /dev/null`
    // reads a baseline-granted device and exits 0.
    assert!(
        ran_successfully(
            || {
                let mut c = Command::new(&granted_bin);
                c.arg("/dev/null");
                c
            },
            &spec
        ),
        "a program in a granted directory must be executable under the sandbox"
    );

    // (2) An identical program in a directory that is *not* granted cannot be
    // executed: Landlock denies the `execve` and the child never starts.
    assert!(
        !ran_successfully(
            || {
                let mut c = Command::new(&offlimits_bin);
                c.arg("/dev/null");
                c
            },
            &spec
        ),
        "a program outside every granted directory must be denied execution"
    );
}

/// A child that attempts a TCP `connect` to `127.0.0.1:port` via bash's
/// `/dev/tcp` pseudo-device, exiting 0 on success and non-zero when the
/// connection is refused or the kernel *denies* it. `/dev/tcp` is a bash
/// builtin (no real filesystem access), so it works under the fs sandbox.
fn connect_cmd(bash: &Path, port: u16) -> Command {
    let mut c = Command::new(bash);
    c.arg("-c")
        .arg(format!("exec 3<>/dev/tcp/127.0.0.1/{port}"));
    c
}

#[test]
fn landlock_confines_tcp_connect_to_the_granted_port() {
    if !landlock_sandbox::is_supported() {
        eprintln!("skipping: the running kernel does not provide Landlock");
        return;
    }
    // TCP net rules are Landlock ABI V4; older kernels enforce fs only, so the
    // connect floor is a no-op there and the deny assertion would not hold.
    if landlock_sandbox::abi_version() < 4 {
        eprintln!("skipping: kernel Landlock ABI < 4 (no TCP net rules)");
        return;
    }
    let Some(bash) = bash() else {
        eprintln!("skipping: no `bash` binary found");
        return;
    };

    // A real loopback listener so an *allowed* connect actually completes; a
    // background thread drains connections so the kernel finishes the handshake.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a loopback listener");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });

    // (1) Granting exactly the listener's port lets the confined child connect.
    // `bind`/`listen` stays unrestricted (this floor is connect-only), so the
    // parent's own listener is unaffected.
    let allow_spec = OsSandboxSpec {
        read_paths: vec![],
        write_paths: vec![],
        exec_programs: vec![],
        connect_ports: vec![port],
    };
    let out = run(connect_cmd(&bash, port), &allow_spec);
    assert!(
        out.status.success(),
        "a connect to the granted port must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // (2) Granting a *different* port means the connect to the listener's port
    // is denied by the kernel, even though the shim is not involved at all.
    let other = port.checked_add(1).unwrap_or(port - 1);
    let deny_spec = OsSandboxSpec {
        read_paths: vec![],
        write_paths: vec![],
        exec_programs: vec![],
        connect_ports: vec![other],
    };
    let out = run(connect_cmd(&bash, port), &deny_spec);
    assert!(
        !out.status.success(),
        "a connect to a port outside the granted set must be denied by Landlock"
    );
}
