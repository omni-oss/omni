#![allow(unsafe_code)]

use std::{
    cell::Cell,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use omni_path_utils::WorkspacePath;
use rustix::io::Errno;

use super::*;
use crate::{
    ContainedRoot,
    error::{ContainmentViolation, InputError},
    open_beneath,
};

const INSIDE: &str = "inside";
const OUTSIDE: &str = "OUTSIDE";

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(outside.join("b")).unwrap();
        fs::write(root.join("a/b/file"), INSIDE).unwrap();
        fs::write(outside.join("b/file"), OUTSIDE).unwrap();
        Self {
            _tmp: tmp,
            root,
            outside,
        }
    }

    /// A fixture, only where the kernel really provides `openat2`. A sandbox
    /// that blocks it (the aarch64 `cross` container, seccomp) gets the named
    /// skip: that lane asserts `Unsupported` instead of claiming containment.
    /// x86_64 Linux must have it, so there a block fails the test.
    fn supported() -> Option<Self> {
        let fixture = Self::new();
        match open_beneath(&fixture.contained(), &wp("a/b/file")) {
            Ok(Some(_)) => Some(fixture),
            Err(error)
                if is_violation(&error, ContainmentViolation::Unsupported) =>
            {
                if cfg!(target_arch = "x86_64") {
                    panic!("x86_64 Linux must provide openat2, got {error:?}");
                }
                let _ = omni_test_capabilities::gate(
                    "openat2",
                    false,
                    "blocked by the sandbox",
                );
                None
            }
            other => panic!("unexpected probe result {other:?}"),
        }
    }

    fn contained(&self) -> ContainedRoot {
        ContainedRoot::open(&self.root).unwrap()
    }
}

fn wp(path: &str) -> WorkspacePath {
    WorkspacePath::parse(path).unwrap()
}

fn read_inside(
    root: &ContainedRoot,
    path: &str,
) -> Result<Option<String>, InputError> {
    use std::io::Read;
    open_beneath(root, &wp(path)).map(|file| {
        file.map(|mut file| {
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();
            text
        })
    })
}

fn open_with<F>(
    fixture: &Fixture,
    path: &str,
    syscall: &F,
) -> Result<String, OpenError>
where
    F: Fn(BorrowedFd<'_>, &str, OFlags, ResolveFlags) -> Result<OwnedFd, Errno>,
{
    use std::io::Read;
    let root = Root::open(&fixture.root).unwrap();
    open_file_with(&root, &wp(path), syscall).map(|mut file| {
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    })
}

fn is_violation(error: &InputError, expected: ContainmentViolation) -> bool {
    matches!(error, InputError::Containment(found) if *found == expected)
}

#[test]
fn reads_a_regular_file_beneath_the_root() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let root = fixture.contained();
    assert_eq!(
        read_inside(&root, "a/b/file").unwrap().as_deref(),
        Some(INSIDE)
    );
}

#[test]
fn missing_and_not_a_directory_are_missing_not_errors() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let root = fixture.contained();
    assert_eq!(read_inside(&root, "a/nope").unwrap(), None);
    assert_eq!(read_inside(&root, "nope/deeper/file").unwrap(), None);
    // ENOTDIR on a parent counts as missing (D6).
    assert_eq!(read_inside(&root, "a/b/file/below").unwrap(), None);
}

#[test]
fn every_symlink_shape_is_a_containment_error() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let (root_dir, outside) = (&fixture.root, &fixture.outside);
    symlink(outside, root_dir.join("link_dir")).unwrap();
    symlink(outside.join("b/file"), root_dir.join("link_file")).unwrap();
    symlink(root_dir.join("a"), root_dir.join("link_inside")).unwrap();
    symlink("gone", root_dir.join("dangling")).unwrap();
    symlink("../../outside/b/file", root_dir.join("a/relative_escape"))
        .unwrap();
    symlink("loop", root_dir.join("loop")).unwrap();
    // The relative link really points at the existing outside file.
    assert_eq!(
        fs::read_to_string(root_dir.join("a/relative_escape")).unwrap(),
        OUTSIDE
    );
    let root = fixture.contained();
    for path in [
        "link_dir/b/file",
        "link_file",
        "link_inside/b/file",
        "dangling",
        "a/relative_escape",
        "loop",
    ] {
        let error = read_inside(&root, path).unwrap_err();
        assert!(
            is_violation(&error, ContainmentViolation::Symlink),
            "{path}: {error:?}"
        );
    }
}

#[test]
fn names_are_checked_before_any_open() {
    let fixture = Fixture::new();
    // The streams-like name really exists on Linux; the view still rejects it.
    fs::write(fixture.root.join("ab:c"), "x").unwrap();
    let root = fixture.contained();
    for path in ["ab:c", "a/CON", "a/nul.txt", "a/b./c", "a/trail "] {
        let error = read_inside(&root, path).unwrap_err();
        assert!(
            is_violation(&error, ContainmentViolation::NameUnsupported),
            "{path}: {error:?}"
        );
    }
}

#[test]
fn the_root_path_is_not_a_file() {
    let fixture = Fixture::new();
    let root = fixture.contained();
    assert!(matches!(
        open_beneath(&root, &WorkspacePath::root()),
        Err(InputError::Io { .. })
    ));
}

#[test]
fn errno_mapping_table() {
    assert_eq!(map_errno(Errno::NOENT), OpenError::Missing);
    assert_eq!(map_errno(Errno::NOTDIR), OpenError::Missing);
    assert_eq!(map_errno(Errno::LOOP), ContainmentViolation::Symlink.into());
    assert_eq!(
        map_errno(Errno::XDEV),
        ContainmentViolation::MountCrossing.into()
    );
    assert_eq!(
        map_errno(Errno::NOSYS),
        ContainmentViolation::Unsupported.into()
    );
    assert_eq!(
        map_errno(Errno::PERM),
        ContainmentViolation::Unsupported.into()
    );
    assert_eq!(
        map_errno(Errno::ACCESS),
        OpenError::Io(std::io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn injected_enosys_and_eperm_are_unsupported_and_read_nothing() {
    let fixture = Fixture::new();
    for errno in [Errno::NOSYS, Errno::PERM] {
        let calls = Cell::new(0);
        let result = open_with(&fixture, "a/b/file", &|_, _, _, _| {
            calls.set(calls.get() + 1);
            Err(errno)
        });
        assert_eq!(result, Err(ContainmentViolation::Unsupported.into()));
        // No fallback: the syscall was asked exactly once and nothing else ran.
        assert_eq!(calls.get(), 1, "{errno:?}");
    }
}

#[test]
fn eagain_retries_a_bounded_number_of_times() {
    let fixture = Fixture::new();
    let calls = Cell::new(0);
    let result = open_with(&fixture, "a/b/file", &|_, _, _, _| {
        calls.set(calls.get() + 1);
        Err(Errno::AGAIN)
    });
    assert_eq!(result, Err(OpenError::Io(std::io::ErrorKind::WouldBlock)));
    assert_eq!(calls.get(), MAX_RETRIES + 1);
}

#[test]
fn eagain_then_success_is_retried() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let calls = Cell::new(0);
    let result =
        open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(Errno::AGAIN)
            } else {
                real_openat2(dirfd, path, oflags, resolve)
            }
        });
    assert_eq!(result.as_deref(), Ok(INSIDE));
}

#[test]
fn the_syscall_gets_the_containment_flags() {
    let fixture = Fixture::new();
    let seen = Cell::new(None);
    let _ = open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
        seen.set(Some((oflags, resolve)));
        real_openat2(dirfd, path, oflags, resolve)
    });
    let (oflags, resolve) = seen.get().unwrap();
    assert_eq!(
        resolve,
        ResolveFlags::BENEATH
            | ResolveFlags::NO_SYMLINKS
            | ResolveFlags::NO_MAGICLINKS
            | ResolveFlags::NO_XDEV
    );
    assert!(
        oflags.contains(OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC)
    );
}

#[test]
fn fifo_opens_without_blocking() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let fifo = fixture.root.join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let root = fixture.contained();
    // Non-blocking open returns; the caller (F10.2b) then rejects it as
    // `NotRegular` from `fstat`.
    assert!(open_beneath(&root, &wp("fifo")).unwrap().is_some());
}

// Deterministic variants: the hook runs between the caller's decision and the
// kernel lookup, which is the window a check-then-open design would lose.

#[test]
fn hook_directory_swapped_for_a_symlink_before_the_lookup() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let result =
        open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
            fs::rename(fixture.root.join("a"), fixture.root.join("a_real"))
                .unwrap();
            symlink(&fixture.outside, fixture.root.join("a")).unwrap();
            real_openat2(dirfd, path, oflags, resolve)
        });
    assert_eq!(result, Err(ContainmentViolation::Symlink.into()));
}

#[test]
fn hook_final_file_swapped_for_a_symlink_before_the_lookup() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let result =
        open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
            let file = fixture.root.join("a/b/file");
            fs::remove_file(&file).unwrap();
            symlink(fixture.outside.join("b/file"), &file).unwrap();
            real_openat2(dirfd, path, oflags, resolve)
        });
    assert_eq!(result, Err(ContainmentViolation::Symlink.into()));
}

#[test]
fn hook_intermediate_directory_moved_out_and_left_there() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let moved = fixture.outside.join("a");
    let result =
        open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
            fs::rename(fixture.root.join("a"), &moved).unwrap();
            replace_file(&moved.join("b/file"), OUTSIDE);
            real_openat2(dirfd, path, oflags, resolve)
        });
    assert_eq!(result, Err(OpenError::Missing));
}

#[test]
fn hook_intermediate_directory_moved_out_and_back() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let moved = fixture.outside.join("a");
    let result =
        open_with(&fixture, "a/b/file", &|dirfd, path, oflags, resolve| {
            fs::rename(fixture.root.join("a"), &moved).unwrap();
            replace_file(&moved.join("b/file"), OUTSIDE);
            replace_file(&moved.join("b/file"), INSIDE);
            fs::rename(&moved, fixture.root.join("a")).unwrap();
            real_openat2(dirfd, path, oflags, resolve)
        });
    assert_eq!(result.as_deref(), Ok(INSIDE));
}

fn replace_file(path: &Path, text: &str) {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).unwrap();
    fs::rename(&tmp, path).unwrap();
}

// Statistical loops. They are not the proof (the deterministic variants and the
// kernel's contract are) but they exercise real concurrency.

const ITERATIONS: usize = 10_000;

fn run_race<S>(
    fixture: &Fixture,
    path: &str,
    want_crossings: bool,
    swapper: S,
) -> usize
where
    S: Fn() + Send + Sync + 'static,
{
    let root = fixture.contained();
    let stop = Arc::new(AtomicBool::new(false));
    let swaps = Arc::new(AtomicUsize::new(0));
    let handle = {
        let (stop, swaps) = (stop.clone(), swaps.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                swapper();
                swaps.fetch_add(1, Ordering::SeqCst);
            }
        })
    };
    let mut found = 0usize;
    let mut crossings = 0usize;
    // At least `ITERATIONS` reads; when the caller needs a kernel escape
    // report, keep going (bounded) so a loaded machine cannot starve it.
    // Make sure the swapper is really running before the reads start, so the
    // reads overlap swaps.
    while swaps.load(Ordering::SeqCst) == 0 && !handle.is_finished() {
        thread::yield_now();
    }
    let swaps_before = swaps.load(Ordering::SeqCst);
    let started = std::time::Instant::now();
    let mut reads = 0usize;
    while reads < ITERATIONS
        || (swaps.load(Ordering::SeqCst) == swaps_before
            && !handle.is_finished()
            && started.elapsed() < std::time::Duration::from_secs(60))
        || (want_crossings
            && crossings == 0
            && started.elapsed() < std::time::Duration::from_secs(60))
    {
        reads += 1;
        match read_inside(&root, path) {
            Ok(Some(text)) => {
                assert_eq!(text, INSIDE, "outside bytes returned");
                found += 1;
            }
            Ok(None) => {}
            Err(error) => {
                // `MountCrossing` here is the kernel's `EXDEV`: it also reports a
                // `RESOLVE_BENEATH` escape it detected during a concurrent
                // rename. Either way the read is refused, which is the point.
                if is_violation(&error, ContainmentViolation::MountCrossing) {
                    crossings += 1;
                }
                assert!(
                    is_violation(&error, ContainmentViolation::Symlink)
                        || is_violation(
                            &error,
                            ContainmentViolation::MountCrossing
                        ),
                    "unexpected error {error:?}"
                );
            }
        }
    }
    // Keep the swapper going until it has had a fair number of rounds, but
    // stop waiting if it died, so its panic is reported by `join`.
    while swaps.load(Ordering::SeqCst) < 100 && !handle.is_finished() {
        thread::yield_now();
    }
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
    assert!(swaps.load(Ordering::SeqCst) >= 100);
    assert!(
        swaps.load(Ordering::SeqCst) > swaps_before,
        "no swap overlapped the reads"
    );
    assert!(found > 0, "every read was refused");
    crossings
}

#[test]
fn race_intermediate_directory_swapped_with_a_symlink() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let (root, outside) = (fixture.root.clone(), fixture.outside.clone());
    run_race(&fixture, "a/b/file", false, move || {
        let (a, real) = (root.join("a"), root.join("a_real"));
        fs::rename(&a, &real).unwrap();
        symlink(&outside, &a).unwrap();
        fs::remove_file(&a).unwrap();
        fs::rename(&real, &a).unwrap();
    });
}

#[test]
fn race_final_file_swapped_with_a_symlink() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let (root, outside) = (fixture.root.clone(), fixture.outside.clone());
    run_race(&fixture, "a/b/file", false, move || {
        let file = root.join("a/b/file");
        let real = root.join("a/b/file_real");
        fs::rename(&file, &real).unwrap();
        symlink(outside.join("b/file"), &file).unwrap();
        fs::remove_file(&file).unwrap();
        fs::rename(&real, &file).unwrap();
    });
}

fn move_out_and_back(
    root: PathBuf,
    outside: PathBuf,
    rel: &'static str,
) -> impl Fn() + Send + Sync {
    move || {
        let (inside_dir, moved) = (root.join(rel), outside.join("moved"));
        fs::rename(&inside_dir, &moved).unwrap();
        replace_file(
            &moved.join(if rel == "a" { "b/file" } else { "file" }),
            OUTSIDE,
        );
        replace_file(
            &moved.join(if rel == "a" { "b/file" } else { "file" }),
            INSIDE,
        );
        fs::rename(&moved, &inside_dir).unwrap();
    }
}

#[test]
fn race_ancestor_moved_out_and_back() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let crossings = run_race(
        &fixture,
        "a/b/file",
        true,
        move_out_and_back(fixture.root.clone(), fixture.outside.clone(), "a"),
    );
    // The kernel's BENEATH escape check (EXDEV) really fired during the
    // lookups; without it these reads could have followed the moved directory.
    assert!(crossings > 0, "the kernel never reported an escape");
}

#[test]
fn race_intermediate_directory_moved_out_and_back() {
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    run_race(
        &fixture,
        "a/b/file",
        false,
        move_out_and_back(fixture.root.clone(), fixture.outside.clone(), "a/b"),
    );
}

// A real seccomp filter that fails `openat2` with EPERM. The test re-executes
// itself with the filter installed in the child; where the filter cannot be
// installed it skips (named), unless the lane requires `seccomp`.

const SECCOMP_CHILD: &str = "OMNI_INPUT_VIEW_SECCOMP_CHILD";
const SECCOMP_ROOT: &str = "OMNI_INPUT_VIEW_SECCOMP_ROOT";

fn install_openat2_filter() -> std::io::Result<()> {
    const SYS_OPENAT2: u32 = 437;
    const RET_ERRNO_EPERM: u32 = 0x0005_0000 | 1;
    const RET_ALLOW: u32 = 0x7fff_0000;
    let mut program = [
        libc::sock_filter {
            code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            jt: 0,
            jf: 0,
            k: 0,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 0,
            jf: 1,
            k: SYS_OPENAT2,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: RET_ERRNO_EPERM,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: RET_ALLOW,
        },
    ];
    let fprog = libc::sock_fprog {
        len: program.len() as u16,
        filter: program.as_mut_ptr(),
    };
    // SAFETY: plain prctl calls with a valid, live `sock_fprog`.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &fprog as *const _,
        ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[test]
fn real_seccomp_filter_blocking_openat2_is_unsupported() {
    use std::os::unix::process::CommandExt;

    if let Some(root) =
        std::env::var_os(SECCOMP_CHILD).and(std::env::var_os(SECCOMP_ROOT))
    {
        let root = ContainedRoot::open(Path::new(&root)).unwrap();
        let error = open_beneath(&root, &wp("a/b/file")).unwrap_err();
        assert!(
            is_violation(&error, ContainmentViolation::Unsupported),
            "{error:?}"
        );
        return;
    }

    // Unfiltered control: where the sandbox already blocks `openat2` the
    // filtered result would prove nothing, so skip (named).
    let Some(fixture) = Fixture::supported() else {
        return;
    };
    let mut command =
        std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "contain::linux::tests::real_seccomp_filter_blocking_openat2_is_unsupported",
            "--test-threads=1",
        ])
        .env(SECCOMP_CHILD, "1")
        .env(SECCOMP_ROOT, &fixture.root);
    // SAFETY: the closure only makes prctl calls on stack data.
    unsafe {
        command.pre_exec(install_openat2_filter);
    }
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "child failed: {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            // `--exact` matching nothing would also exit 0.
            assert!(stdout.contains("1 passed"), "child ran nothing: {stdout}");
        }
        Err(error) => {
            // Skips, named, unless the lane requires `seccomp`.
            let _ = omni_test_capabilities::gate("seccomp", false, error);
        }
    }
}
