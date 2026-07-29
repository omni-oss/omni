//! Live end-to-end test of the Windows AppContainer backend: it spawns **real
//! child processes** confined by [`appcontainer_sandbox::spawn`] and proves the
//! kernel denies filesystem access outside the granted subtrees while permitting
//! it inside.
//!
//! This is the OS-sandbox analog of `landlock_spawn.rs` (Linux). It is
//! Windows-only and **skips** (does not fail) when the host does not provide a
//! usable AppContainer facility, so it is safe in any CI.

#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use omni_capability_enforcement::{OsSandboxSpec, appcontainer_sandbox};

/// Creating omni's AppContainer profile is not safe to run concurrently: two
/// threads racing `CreateAppContainerProfile` on the same name can fail with
/// `ERROR_BAD_ENVIRONMENT` (HRESULT 0x8007000A) instead of the expected
/// `ERROR_ALREADY_EXISTS`. The live spawn tests share omni's single profile
/// name, so serialize their confined spawns behind this guard.
static SPAWN_GUARD: Mutex<()> = Mutex::new(());

/// Whether the confined-spawn body should run. Returns `true` when AppContainer
/// is available. When it is not, the default is to **skip** (return `false`) so
/// the suite stays green on hosts/CI without the facility — *unless*
/// `OMNI_REQUIRE_OS_SANDBOX` is set, in which case an unavailable facility is a
/// hard failure. That makes the otherwise-silent soft-skip un-skippable in a CI
/// that is *supposed* to exercise real confinement (a green run there then
/// genuinely proves the sandbox worked, not merely that it was absent).
fn should_run_confined() -> bool {
    if appcontainer_sandbox::is_supported() {
        return true;
    }
    assert!(
        std::env::var_os("OMNI_REQUIRE_OS_SANDBOX").is_none(),
        "OMNI_REQUIRE_OS_SANDBOX is set but this host provides no usable \
         AppContainer facility"
    );
    eprintln!("skipping: the host does not provide AppContainer");
    false
}

/// First existing candidate path, or `None` (test then skips).
fn first_existing(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// A fresh, unique scratch directory for a test.
///
/// Deliberately rooted under `CARGO_TARGET_TMPDIR` (inside the repo's `target/`)
/// rather than the system temp dir: a real confined runtime launch grants the
/// whole platform temp dir writable to the sandbox (runtimes stage temp files
/// there), and that grant lands on the *persistent* container profile's SID, so
/// a `%TEMP%`-based "denied" path can be spuriously reachable on a machine that
/// has ever run a confined generator. `target/` is never granted, keeping the
/// negative assertions sound regardless of prior runs.
fn scratch_dir() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let unique = format!(
        "appcontainer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let dir = base.join(unique);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `cmd.exe` from the system directory, used as a real, dynamically-linked
/// program whose loader/DLLs live under `C:\Windows` (granted to AppContainers
/// by default), so it can start inside the container.
fn cmd_exe() -> Option<PathBuf> {
    let system_root = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".to_string());
    first_existing(&[
        &format!(r"{system_root}\System32\cmd.exe"),
        r"C:\Windows\System32\cmd.exe",
    ])
}

/// Spawn `command` confined by `spec`, wait, and return its output. The ACL
/// guard is held until the child has fully exited (mirroring the runner), so the
/// grants stay in place for the child's whole life and are revoked afterwards.
fn run(mut command: Command, spec: &OsSandboxSpec) -> std::process::Output {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, _acl_guard) = appcontainer_sandbox::spawn(&mut command, spec)
        .expect("spawning the confined child failed");
    child.wait_with_output().expect("waiting on child failed")
}

/// Grant read to the runtime's own program directory so it (and its DLLs) can
/// start under the container, plus whatever the test wants to allow.
fn spec_with(read: Vec<PathBuf>, write: Vec<PathBuf>) -> OsSandboxSpec {
    OsSandboxSpec {
        read_paths: read,
        write_paths: write,
        exec_programs: Vec::new(),
        connect_ports: Vec::new(),
    }
}

#[test]
fn appcontainer_confines_reads_to_the_granted_subtree() {
    if !should_run_confined() {
        return;
    }
    let Some(cmd) = cmd_exe() else {
        eprintln!("skipping: no cmd.exe found");
        return;
    };

    // Serialize profile creation across the live spawn tests (see `SPAWN_GUARD`).
    let _guard = SPAWN_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let dir = scratch_dir();
    let allowed = dir.join("allowed");
    let secret = dir.join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();
    let ok = allowed.join("ok.txt");
    let hidden = secret.join("hidden.txt");
    fs::write(&ok, b"hello").unwrap();
    fs::write(&hidden, b"top secret").unwrap();

    // Grant read only to the `allowed` subtree; `secret` is deliberately not
    // granted. `type` (cmd builtin) prints a file's contents.
    let spec = spec_with(vec![allowed.clone()], vec![]);

    // (1) A read inside the granted subtree succeeds. The child's working
    // directory is the granted subtree (the parent's CWD — the repo checkout —
    // is not accessible to the container).
    let mut c = Command::new(&cmd);
    c.current_dir(&allowed).arg("/c").arg("type").arg(&ok);
    let out = run(c, &spec);
    assert!(
        out.status.success(),
        "an allowed read must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello"),
        "the allowed read did not return the file contents; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // (2) A read outside it is denied by the container: `type` cannot open the
    // file, so it exits non-zero.
    let mut c = Command::new(&cmd);
    c.current_dir(&allowed).arg("/c").arg("type").arg(&hidden);
    let out = run(c, &spec);
    assert!(
        !out.status.success(),
        "a read outside the granted subtree must be denied by AppContainer"
    );
}

#[test]
fn appcontainer_confines_writes_to_the_granted_subtree() {
    if !should_run_confined() {
        return;
    }
    let Some(cmd) = cmd_exe() else {
        eprintln!("skipping: no cmd.exe found");
        return;
    };

    // Serialize profile creation across the live spawn tests (see `SPAWN_GUARD`).
    let _guard = SPAWN_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let dir = scratch_dir();
    let allowed = dir.join("allowed");
    let secret = dir.join("secret");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&secret).unwrap();

    // Both dirs readable (so the child can start with its CWD there), only
    // `allowed` writable.
    let spec =
        spec_with(vec![allowed.clone(), secret.clone()], vec![allowed.clone()]);

    // Redirect `echo` output into `name`, run from `dir` as the working
    // directory so the target is a simple relative path (no quoting).
    let write_cmd = |dir: &std::path::Path, name: &str| {
        let mut c = Command::new(&cmd);
        c.current_dir(dir).arg("/c").arg(format!("echo hi>{name}"));
        c
    };

    // (1) A write inside the granted subtree succeeds and lands on disk.
    let out = run(write_cmd(&allowed, "w.txt"), &spec);
    assert!(
        out.status.success(),
        "an allowed write must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        allowed.join("w.txt").exists(),
        "the allowed write did not reach disk"
    );

    // (2) A write outside it is denied, and nothing is created. The child still
    // starts (its CWD `secret` is readable) but cannot create the file there.
    let _ = run(write_cmd(&secret, "w.txt"), &spec);
    assert!(
        !secret.join("w.txt").exists(),
        "the denied write must not have created a file"
    );
}

#[test]
fn is_supported_is_stable() {
    // A pure probe that neither confines the test process nor leaks: it must be
    // callable repeatedly and agree with itself.
    let a = appcontainer_sandbox::is_supported();
    let b = appcontainer_sandbox::is_supported();
    assert_eq!(a, b);
}

#[test]
fn appcontainer_child_can_launch_a_grandchild_and_capture_its_output() {
    // A confined runtime does not just touch files: it spawns *grandchildren*
    // (the whole point of the `process` capability). This proves the container
    // does not silently break child creation or handle inheritance — a confined
    // `cmd.exe` launches a grandchild `whoami.exe` and captures its stdout over
    // the inherited pipe. `whoami` lives in `System32` (ambient read/execute for
    // AppContainers), so no `exec_programs` grant is needed to reach it.
    //
    // (Note: a grandchild *`cmd.exe`* specifically is refused by the OS inside
    // an AppContainer — CreateProcess returns access-denied even though its ACL
    // grants ALL APPLICATION PACKAGES — whereas ordinary programs like `whoami`
    // and `git` spawn fine. That `cmd`-as-shell limitation is why the e2e
    // `runs a shell invocation` stays skipped on Windows.)
    if !should_run_confined() {
        return;
    }
    let Some(cmd) = cmd_exe() else {
        eprintln!("skipping: no cmd.exe found");
        return;
    };

    // Serialize profile creation across the live spawn tests (see `SPAWN_GUARD`).
    let _guard = SPAWN_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let dir = scratch_dir();
    let allowed = dir.join("allowed");
    fs::create_dir(&allowed).unwrap();
    let spec = spec_with(vec![allowed.clone()], vec![allowed.clone()]);

    // Outer confined `cmd` runs `<cmd> /c whoami`, i.e. it must itself
    // CreateProcess a grandchild `whoami` inside the container and relay its
    // output back over the pipe.
    let mut c = Command::new(&cmd);
    c.current_dir(&allowed).arg("/c").arg("whoami");
    let out = run(c, &spec);
    assert!(
        out.status.success(),
        "the confined child could not launch a grandchild; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "the grandchild produced no output over the inherited pipe"
    );
}

// NOTE on `exec_programs` and confined grandchildren: the backend resolves an
// allowed program name and grants its real binary directory read/execute (see
// `appcontainer_sandbox::program_dirs`, unit-tested there). A confined process
// can then spawn *ambient* grandchildren (System32, exercised by the test
// above). Spawning a *user-installed* binary as a grandchild is not covered by
// a live test here: it proved environment-dependent — on a locked-down host a
// real user-dir binary (e.g. Git for Windows) is refused by the OS with
// access-denied at CreateProcess even with correct, identical ACL grants and
// fully-traversable ancestors, which points to a host execution policy
// (WDAC/AppLocker) or a deeper AppContainer restriction outside omni's control.
// The grant logic omni *is* responsible for is asserted by the `program_dirs`
// unit tests instead of a flaky live launch.
