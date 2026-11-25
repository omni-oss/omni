use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use omni_path_utils::WorkspacePath;
use omni_test_capabilities::Capability;

use super::*;
use crate::{ContainedRoot, error::InputError, open_beneath};

const INSIDE: &str = "inside";
const OUTSIDE: &str = "OUTSIDE";

trait At {
    fn at(&self, relative: &str) -> PathBuf;
}

impl At for Path {
    fn at(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.to_path_buf(), |path, part| path.join(part))
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // Not canonicalized: a verbatim `\\?\` path does not accept `/` joins or
        // `mklink`. `ContainedRoot::open` canonicalizes the root itself.
        let base = tmp.path().to_path_buf();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(root.at("a/b")).unwrap();
        fs::create_dir_all(outside.join("b")).unwrap();
        fs::write(root.at("a/b/file"), INSIDE).unwrap();
        fs::write(outside.at("b/file"), OUTSIDE).unwrap();
        Self {
            _tmp: tmp,
            root,
            outside,
        }
    }

    fn contained(&self) -> ContainedRoot {
        ContainedRoot::open(&self.root).unwrap()
    }
}

fn wp(path: &str) -> WorkspacePath {
    WorkspacePath::parse(path).unwrap()
}

fn read_text(mut file: File) -> String {
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

fn read_inside(
    root: &ContainedRoot,
    path: &str,
) -> Result<Option<String>, InputError> {
    open_beneath(root, &wp(path)).map(|file| file.map(read_text))
}

fn junction(link: &Path, target: &Path) {
    let output = Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn is_reparse(error: &InputError) -> bool {
    matches!(
        error,
        InputError::Containment(ContainmentViolation::ReparsePoint)
    )
}

fn raw_walk(
    fixture: &Fixture,
    path: &str,
    hook: &mut dyn FnMut(Step),
    flag: u32,
) -> Result<String, OpenError> {
    let root = Root::open(&fixture.root).unwrap();
    open_file_with_flags(&root, &wp(path), hook, flag).map(read_text)
}

#[test]
fn reads_a_regular_file_beneath_the_root() {
    let fixture = Fixture::new();
    let root = fixture.contained();
    assert_eq!(
        read_inside(&root, "a/b/file").unwrap().as_deref(),
        Some(INSIDE)
    );
}

#[test]
fn missing_and_not_a_directory_are_missing_not_errors() {
    let fixture = Fixture::new();
    let root = fixture.contained();
    assert_eq!(read_inside(&root, "a/nope").unwrap(), None);
    assert_eq!(read_inside(&root, "nope/deeper/file").unwrap(), None);
    assert_eq!(read_inside(&root, "a/b/file/below").unwrap(), None);
}

#[test]
fn junctions_are_rejected_and_never_skipped() {
    let fixture = Fixture::new();
    junction(&fixture.root.join("link_dir"), &fixture.outside);
    junction(&fixture.root.at("a/link_inside"), &fixture.root.at("a/b"));
    let root = fixture.contained();
    for path in [
        "link_dir",
        "link_dir/b/file",
        "a/link_inside/file",
        "a/link_inside",
    ] {
        let error = read_inside(&root, path).unwrap_err();
        assert!(is_reparse(&error), "{path}: {error:?}");
    }
}

#[test]
fn symlinks_are_rejected() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let fixture = Fixture::new();
    if !omni_test_capabilities::require(Capability::Symlink, &fixture.root) {
        return;
    }
    symlink_dir(&fixture.outside, fixture.root.join("link_dir")).unwrap();
    symlink_file(fixture.outside.at("b/file"), fixture.root.join("link_file"))
        .unwrap();
    symlink_file(fixture.root.join("gone"), fixture.root.join("dangling"))
        .unwrap();
    let root = fixture.contained();
    for path in ["link_dir", "link_dir/b/file", "link_file", "dangling"] {
        let error = read_inside(&root, path).unwrap_err();
        assert!(is_reparse(&error), "{path}: {error:?}");
    }
}

#[test]
fn names_are_rejected_by_d9_not_reported_as_missing() {
    let fixture = Fixture::new();
    // A real alternate data stream on a real file.
    fs::write(fixture.root.at("a/b/file:ads"), "stream").unwrap();
    let root = fixture.contained();
    for path in [
        "a/b/file:ads",
        "a/CON",
        "a/nul.txt",
        "a/COM1",
        "a/b./c",
        "a/trail ",
    ] {
        let error = read_inside(&root, path).unwrap_err();
        assert!(
            matches!(
                error,
                InputError::Containment(ContainmentViolation::NameUnsupported)
            ),
            "{path}: {error:?}"
        );
    }
}

// Held-handle rename rejection: the boundary the whole Windows design rests on.
// If a rename succeeds here, Windows becomes `Unsupported` (re-planning trigger).

fn assert_rename_blocked(from: &Path, to: &Path) {
    let result = fs::rename(from, to);
    assert!(
        result.is_err(),
        "renaming the held directory {} succeeded",
        from.display()
    );
    assert!(from.exists(), "{} disappeared", from.display());
}

#[test]
fn held_ancestors_cannot_be_renamed_and_move_out_and_back_cannot_run() {
    let fixture = Fixture::new();
    let (a, b) = (fixture.root.join("a"), fixture.root.at("a/b"));
    let moved = fixture.outside.join("moved");
    let mut seen = Vec::new();
    let result = raw_walk(
        &fixture,
        "a/b/file",
        &mut |step| {
            seen.push(step);
            match step {
                Step::Retained(0) => {
                    assert_rename_blocked(&a, &moved);
                }
                Step::Retained(1) => {
                    assert_rename_blocked(&b, &moved);
                    assert_rename_blocked(&a, &moved);
                }
                Step::Opened => {
                    assert_rename_blocked(&b, &moved);
                    assert_rename_blocked(&a, &moved);
                    // The file itself is held without FILE_SHARE_DELETE.
                    assert_rename_blocked(
                        &b.join("file"),
                        &fixture.outside.join("file"),
                    );
                }
                Step::Retained(_) => unreachable!(),
            }
        },
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(result.as_deref(), Ok(INSIDE));
    assert_eq!(seen, [Step::Retained(0), Step::Retained(1), Step::Opened]);
    assert!(!moved.exists());
}

#[test]
fn the_held_root_cannot_be_renamed() {
    let fixture = Fixture::new();
    let renamed = fixture.root.with_file_name("renamed");
    let result = raw_walk(
        &fixture,
        "a/b/file",
        &mut |step| {
            if step == Step::Retained(0) {
                assert_rename_blocked(&fixture.root, &renamed);
            }
        },
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(result.as_deref(), Ok(INSIDE));
}

// Deterministic variants: the state of the next component changes between two
// component resolutions.

#[test]
fn hook_next_component_swapped_for_a_junction_after_the_parent_is_held() {
    let fixture = Fixture::new();
    let result = raw_walk(
        &fixture,
        "a/b/file",
        &mut |step| {
            if step == Step::Retained(0) {
                fs::rename(fixture.root.at("a/b"), fixture.root.at("a/b_real"))
                    .unwrap();
                junction(&fixture.root.at("a/b"), &fixture.outside.join("b"));
            }
        },
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(result, Err(ContainmentViolation::ReparsePoint.into()));
}

#[test]
fn hook_final_directory_swapped_for_a_junction_before_the_open() {
    let fixture = Fixture::new();
    let result = raw_walk(
        &fixture,
        "a/b",
        &mut |step| {
            if step == Step::Retained(0) {
                fs::rename(fixture.root.at("a/b"), fixture.root.at("a/b_real"))
                    .unwrap();
                junction(&fixture.root.at("a/b"), &fixture.outside);
            }
        },
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(result, Err(ContainmentViolation::ReparsePoint.into()));
}

#[test]
fn hook_final_file_swapped_for_a_symlink_before_the_open() {
    use std::os::windows::fs::symlink_file;
    let fixture = Fixture::new();
    if !omni_test_capabilities::require(Capability::Symlink, &fixture.root) {
        return;
    }
    let result = raw_walk(
        &fixture,
        "a/b/file",
        &mut |step| {
            if step == Step::Retained(1) {
                let file = fixture.root.at("a/b/file");
                fs::remove_file(&file).unwrap();
                symlink_file(fixture.outside.at("b/file"), &file).unwrap();
            }
        },
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(result, Err(ContainmentViolation::ReparsePoint.into()));
}

/// FILE_FLAG_OPEN_REPARSE_POINT is load-bearing. Without it, cap-std notices
/// the final-component symlink itself and follows it (`manually::open`), so
/// the read succeeds and the reparse-point rejection never happens.
#[test]
fn omitting_the_reparse_flag_lets_a_symlink_be_followed() {
    use std::os::windows::fs::symlink_file;
    let fixture = Fixture::new();
    if !omni_test_capabilities::require(Capability::Symlink, &fixture.root) {
        return;
    }
    // A relative target: cap-std rejects an absolute one as an escape attempt.
    symlink_file("file", fixture.root.at("a/b/link_file")).unwrap();

    let with_flag = raw_walk(
        &fixture,
        "a/b/link_file",
        &mut |_| {},
        FILE_FLAG_OPEN_REPARSE_POINT,
    );
    assert_eq!(with_flag, Err(ContainmentViolation::ReparsePoint.into()));

    let without_flag = raw_walk(&fixture, "a/b/link_file", &mut |_| {}, 0);
    assert_eq!(
        without_flag.as_deref(),
        Ok(INSIDE),
        "the symlink was not followed without the flag"
    );
}

#[test]
fn final_path_check_rejects_a_handle_outside_the_root() {
    let fixture = Fixture::new();
    let root = Root::open(&fixture.root).unwrap();
    let outside = File::open(fixture.outside.at("b/file")).unwrap();
    let found = sys::final_path(&outside).unwrap();
    assert!(!is_under(&found, &root.final_path));
    let inside = File::open(fixture.root.at("a/b/file")).unwrap();
    assert!(is_under(
        &sys::final_path(&inside).unwrap(),
        &root.final_path
    ));
}

// Statistical loop: a thread swaps a component between a real directory and a
// junction to an outside directory while the reader loops. Most swaps fail
// while the reader holds handles; that is the boundary working.

const ITERATIONS: usize = 10_000;

#[test]
fn race_intermediate_directory_swapped_with_a_junction() {
    let fixture = Fixture::new();
    let root = fixture.contained();
    let (root_dir, outside) = (fixture.root.clone(), fixture.outside.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let handle = {
        let stop = stop.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (a, real) = (root_dir.join("a"), root_dir.join("a_real"));
                if fs::rename(&a, &real).is_ok() {
                    junction(&a, &outside);
                    retry(|| fs::remove_dir(&a));
                    retry(|| fs::rename(&real, &a));
                }
            }
        })
    };
    for _ in 0..ITERATIONS {
        if let Ok(Some(text)) = read_inside(&root, "a/b/file") {
            assert_eq!(text, INSIDE, "outside bytes returned");
        }
    }
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}

/// Sharing violations are expected while the reader holds a handle.
fn retry(mut operation: impl FnMut() -> std::io::Result<()>) {
    for _ in 0..10_000 {
        if operation().is_ok() {
            return;
        }
        thread::yield_now();
    }
    panic!("operation kept failing");
}
