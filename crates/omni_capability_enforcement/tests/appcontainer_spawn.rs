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
        confine: false,
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

// ── Confined 2a transport spike ──────────────────────────────────────────────
//
// De-risks strategy A's dedicated *synchronous* broker-read channel: an
// in-thread Node loader hook must block on a second pipe while the host serves
// module bytes over it. Because an AppContainer child runs at low integrity,
// Mandatory Integrity Control (MIC) denies it *writing up* to a host-created
// pipe by default. This proves empirically (1) whether a DACL ACE for the
// container SID alone lets the confined child write to the pipe or a Low
// integrity SACL label is additionally required, (2) that the confined child's
// write survives MIC, and (3) that a single pipe instance suffices for the
// request/response round trip.
const SPIKE_TOKEN: &str = "OMNISPIKEOK";

use std::os::windows::ffi::OsStrExt as _;

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, OPEN_EXISTING, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};

/// `PIPE_ACCESS_DUPLEX` (`winbase.h`): a read/write pipe. Defined locally as
/// `windows-sys` groups it under a feature the pipe functions do not need.
const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;

/// `GENERIC_WRITE` (`winnt.h`), defined locally to avoid a feature dependency.
const GENERIC_WRITE: u32 = 0x4000_0000;
/// `GENERIC_READ` (`winnt.h`), defined locally to avoid a feature dependency.
const GENERIC_READ: u32 = 0x8000_0000;

/// The confined pipe *client*, re-exec'd as a child of the spike test.
///
/// A named-pipe client must open the pipe with `OPEN_EXISTING`; `cmd.exe`
/// redirection uses a *create* disposition that NPFS rejects outright, so the
/// client cannot be a shell one-liner. Instead the spike re-execs this very
/// test binary (filtered to just this test) inside the AppContainer, handing it
/// the pipe name via `OMNI_2A_SPIKE_PIPE`. With the env unset (an ordinary test
/// run) it is a no-op. With it set it opens the pipe for write, writes
/// [`SPIKE_TOKEN`], and exits `0` on a successful write / `2` on denial — that
/// exit code is the parent's write-up signal.
#[test]
fn spike_2a_pipe_client() {
    let Ok(name) = std::env::var("OMNI_2A_SPIKE_PIPE") else {
        return;
    };
    let wname = to_wide(&name);

    // Probe read-open and write-open separately so the parent can tell a total
    // access-denied (DACL / object-namespace) apart from a MIC write-up denial
    // (read-up is allowed by default, write-up is not). Exit code encodes it:
    //   0  = write-open succeeded and the token was written (write-up works)
    //   3  = read-open succeeded but write-open denied (pure MIC write-up block)
    //   2  = neither open succeeded (DACL / namespace denies access outright)
    let open = |access: u32| -> bool {
        // SAFETY: FFI. `wname` is a valid null-terminated path; opening an
        // existing pipe with the given access, no sharing, no template.
        let h = unsafe {
            CreateFileW(
                wname.as_ptr(),
                access,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut wrote = true;
        if access & GENERIC_WRITE != 0 {
            let bytes = SPIKE_TOKEN.as_bytes();
            let mut written: u32 = 0;
            // SAFETY: FFI on the live handle; `bytes` readable for its length.
            let ok = unsafe {
                WriteFile(
                    h,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            wrote = ok != 0 && written as usize == bytes.len();
        }
        // SAFETY: done with the handle.
        unsafe { CloseHandle(h) };
        wrote
    };

    if open(GENERIC_WRITE) {
        std::process::exit(0);
    }
    let read_ok = open(GENERIC_READ);
    std::process::exit(if read_ok { 3 } else { 2 });
}

/// Encode a string as a null-terminated UTF-16 buffer for the `*W` APIs.
fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Build a self-relative security descriptor from an SDDL string.
fn sddl_to_descriptor(sddl: &str) -> PSECURITY_DESCRIPTOR {
    let wide = to_wide(sddl);
    let mut psd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: FFI. `wide` is a valid null-terminated SDDL string; revision 1 is
    // the only defined SDDL revision; `psd` receives a `LocalAlloc`'d descriptor
    // on success (freed by the caller after `CreateNamedPipeW` copies it).
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut psd,
            std::ptr::null_mut(),
        )
    };
    assert!(
        ok != 0 && !psd.is_null(),
        "ConvertStringSecurityDescriptorToSecurityDescriptorW({sddl}) failed: {}",
        std::io::Error::last_os_error()
    );
    psd
}

/// Create a single-instance byte-mode named pipe secured by `sddl`, returning
/// its server handle. The handle's own access is fixed at creation (full, to
/// the creator), so `sddl` governs only what a *client* (the confined child)
/// may do when it opens the pipe.
fn create_secured_pipe(name: &str, sddl: &str) -> std::io::Result<HANDLE> {
    let psd = sddl_to_descriptor(sddl);
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: psd,
        bInheritHandle: 0,
    };
    let wname = to_wide(name);
    // SAFETY: FFI. `wname` is a valid pipe name; `sa` carries the descriptor for
    // the pipe's lifetime of this call (the object copies it), and is a single
    // instance, blocking byte pipe.
    let handle = unsafe {
        CreateNamedPipeW(
            wname.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            512,
            512,
            0,
            &sa,
        )
    };
    // The object took its own copy of the descriptor at creation; free ours.
    // SAFETY: `psd` is a `LocalAlloc`'d descriptor no longer referenced.
    unsafe { LocalFree(psd as HLOCAL) };
    let _ = &mut sa;
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    Ok(handle)
}

/// Run one spike case: create a pipe secured with a DACL ACE for `dacl_sid`
/// (optionally also carrying an integrity SACL label at `label`), spawn the
/// **confined** re-exec'd [`spike_2a_pipe_client`] to write [`SPIKE_TOKEN`] up
/// to the pipe, and read it back on the host.
///
/// Returns `Ok(true)` when the confined child wrote up and the host read the
/// token back (MIC write-up succeeded), `Ok(false)` when the child ran but was
/// denied (non-zero exit, nothing readable), or `Err` when the case could not
/// even be set up / the confined child could not be launched (the test then
/// treats that as an inconclusive skip rather than a failure).
fn confined_pipe_write_survives(
    dacl_sid: &str,
    label: Option<&str>,
) -> std::io::Result<bool> {
    let name = format!(
        r"\\.\pipe\omni-2a-spike-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    // DACL: allow generic-all to `dacl_sid`. With a label, set the object's
    // integrity via an `ML` label ACE (no-write-up policy) to the given level,
    // so a subject at or above that level is not blocked by MIC write-up.
    let sddl = match label {
        Some(level) => format!("D:(A;;GA;;;{dacl_sid})S:(ML;;NW;;;{level})"),
        None => format!("D:(A;;GA;;;{dacl_sid})"),
    };

    let pipe = create_secured_pipe(&name, &sddl)?;

    // A pipe client must open with OPEN_EXISTING; `cmd.exe` redirection uses a
    // create disposition NPFS rejects, so the client is this test binary
    // re-exec'd confined. Copy it into a small granted scratch dir so the child
    // needs only that dir (its runtime DLLs are ambient System32), keeping the
    // grant tiny instead of granting the whole `deps/` tree.
    let dir = scratch_dir();
    let mut c = match spike_client_command(&dir, &name) {
        Ok(c) => c,
        Err(e) => {
            // SAFETY: `pipe` is a live handle from `create_secured_pipe`.
            unsafe { CloseHandle(pipe) };
            return Err(e);
        }
    };

    let spec = spec_with(vec![dir.clone()], vec![dir.clone()]);

    let (child, guard) = match appcontainer_sandbox::spawn(&mut c, &spec) {
        Ok(v) => v,
        Err(e) => {
            // SAFETY: `pipe` is a live handle.
            unsafe { CloseHandle(pipe) };
            return Err(e);
        }
    };
    let out = child.wait_with_output()?;
    drop(guard);
    // SAFETY: `pipe` is a live handle; the host need not accept the connection
    // — the confined client's own exit code is the write-up verdict.
    unsafe { CloseHandle(pipe) };

    // Exit 0 = the confined client opened the pipe for write AND its WriteFile
    // succeeded (the write-up survived). 3 = it could open for read but not
    // write (a MIC write-up block). 2 = it could not open the pipe at all (the
    // DACL / object namespace denied access outright). Anything else = the
    // confined harness itself failed to run.
    match out.status.code() {
        Some(0) => Ok(true),
        Some(code) => {
            eprintln!(
                "  child exit={code} stdout={:?} stderr={:?}",
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim(),
            );
            Ok(false)
        }
        None => Ok(false),
    }
}

/// Copy this test binary into `dir` as a standalone pipe client and return a
/// [`Command`] that re-execs it filtered to [`spike_2a_pipe_client`], wired to
/// `name` via the environment and with piped stdio.
fn spike_client_command(
    dir: &std::path::Path,
    name: &str,
) -> std::io::Result<Command> {
    let client = dir.join("spike-client.exe");
    std::fs::copy(std::env::current_exe()?, &client)?;
    let mut c = Command::new(client);
    c.current_dir(dir)
        .env("OMNI_2A_SPIKE_PIPE", name)
        .args(["--exact", "spike_2a_pipe_client", "--nocapture"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(c)
}

#[test]
fn confined_2a_transport_low_integrity_write_up() {
    if !should_run_confined() {
        return;
    }

    // Serialize profile creation across the live spawn tests (see `SPAWN_GUARD`).
    let _guard = SPAWN_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let sid = appcontainer_sandbox::container_sid_string()
        .expect("derive omni's container SID string");

    // "ALL APPLICATION PACKAGES" (S-1-15-2-1), the coarse principal every
    // AppContainer carries, used as a control to separate a DACL-principal
    // problem from a Mandatory-Integrity-Control write-up problem.
    const ALL_APP_PACKAGES: &str = "S-1-15-2-1";
    // Integrity levels as SDDL label SIDs: Low and Untrusted.
    const UNTRUSTED: &str = "S-1-16-0";

    let combos: &[(&str, &str, Option<&str>)] = &[
        ("container-sid, no label", &sid, None),
        ("container-sid, Low label", &sid, Some("LW")),
        ("container-sid, Untrusted label", &sid, Some(UNTRUSTED)),
        ("all-app-packages, no label", ALL_APP_PACKAGES, None),
        ("all-app-packages, Low label", ALL_APP_PACKAGES, Some("LW")),
        (
            "all-app-packages, Untrusted label",
            ALL_APP_PACKAGES,
            Some(UNTRUSTED),
        ),
    ];

    let mut any_worked = false;
    for (label, dacl_sid, integrity) in combos {
        match confined_pipe_write_survives(dacl_sid, *integrity) {
            Ok(worked) => {
                eprintln!("2a spike: {label:<34} write-up = {worked}");
                any_worked |= worked;
            }
            Err(e) => {
                // The confined child could not even be launched (e.g. a host
                // execution policy refuses a confined user-dir binary). That is
                // inconclusive, not a proof of failure: skip rather than fail.
                eprintln!(
                    "skipping 2a spike: could not run the confined pipe \
                     client ({label}): {e}"
                );
                return;
            }
        }
    }

    // FINDING (recorded in RFC 0006 §5.5 + decision log): on this host a child
    // confined by omni's AppContainer `spawn` is denied *opening* a host-created
    // named pipe outright — the client's `CreateFile` fails for BOTH read and
    // write (exit 2), in every configuration, including a DACL granting
    // `ALL APPLICATION PACKAGES` and a Low/Untrusted integrity SACL label. So
    // the barrier is not MIC write-up (which a label would clear) but access to
    // the pipe object itself. The unconfined control
    // (`spike_2a_unconfined_pipe_roundtrip_control`) proves the pipe/client/read
    // mechanics are sound, so this is a genuine platform finding.
    //
    // Consequence: the naive strategy-A transport (a plain host-created named
    // pipe the confined child opens) is NOT viable as designed. The Phase 9
    // gate is therefore NOT satisfied; strategy A stays unimplemented and is
    // re-scoped to a follow-up RFC (which must solve container-reachable IPC,
    // e.g. ALPC or a container-namespace pipe).
    //
    // This is pinned as a characterization: if the confined child ever DOES
    // reach the pipe on some host/Windows build, the assertion fires and we
    // revisit the strategy-A gate rather than silently carrying a stale finding.
    assert!(
        !any_worked,
        "a confined AppContainer child unexpectedly reached a host named pipe — \
         the strategy A transport may now be viable; revisit RFC 0006 §5.5 and \
         the Phase 9 gate"
    );
    eprintln!(
        "2a spike finding: a confined child cannot open a host named pipe \
         (denied at CreateFile for read AND write, all DACL/label combos) — \
         strategy A's naive host-pipe transport is not viable; deferred."
    );
}

#[test]
fn spike_2a_unconfined_pipe_roundtrip_control() {
    // Proves the pipe + re-exec client + host-read mechanics are sound, so a
    // *confined* denial in the test above is a genuine AppContainer/MIC finding
    // rather than a bug in this harness. Runs the client UNCONFINED against a
    // pipe whose DACL grants Everyone (`WD`).
    let name = format!(
        r"\\.\pipe\omni-2a-ctl-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    let pipe = create_secured_pipe(&name, "D:(A;;GA;;;WD)")
        .expect("create control pipe");
    let dir = scratch_dir();
    let mut c =
        spike_client_command(&dir, &name).expect("build control client");
    let out = c.output().expect("run unconfined control client");
    // SAFETY: `pipe` is a live handle; the client's exit code is the verdict.
    unsafe { CloseHandle(pipe) };
    assert!(
        out.status.success(),
        "unconfined client could not open+write the pipe (exit {:?}) — the \
         harness/pipe mechanics are broken: stdout={:?} stderr={:?}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim(),
    );
}
