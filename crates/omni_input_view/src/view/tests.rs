use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::*;
use crate::{
    auth::{AuthError, DenyCause, DenyReason},
    digest::DigestSink,
};

struct Policy {
    deny: Mutex<HashSet<String>>,
    fail: Mutex<HashSet<String>>,
    calls: Mutex<Vec<String>>,
}

impl Policy {
    fn allow_all() -> Arc<Self> {
        Arc::new(Self {
            deny: Mutex::default(),
            fail: Mutex::default(),
            calls: Mutex::default(),
        })
    }

    fn deny(&self, path: &str) {
        self.deny.lock().unwrap().insert(path.to_string());
    }

    fn fail(&self, path: &str) {
        self.fail.lock().unwrap().insert(path.to_string());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl InputAuthorizer for Policy {
    fn authorize_read(
        &self,
        path: &WorkspacePath,
    ) -> Result<AuthDecision, AuthError> {
        self.calls.lock().unwrap().push(path.to_string());
        if self.fail.lock().unwrap().contains(path.as_str()) {
            return Err(AuthError);
        }
        if self.deny.lock().unwrap().contains(path.as_str()) {
            return Ok(AuthDecision::Deny(DenyReason {
                path: path.clone(),
                cause: DenyCause::ExplicitDeny,
            }));
        }
        Ok(AuthDecision::Allow)
    }
}

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
    policy: Arc<Policy>,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.at("a/b")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.at("a/b/file"), "inside").unwrap();
        fs::write(outside.at("secret"), "OUTSIDE").unwrap();
        Self {
            _tmp: tmp,
            root,
            outside,
            policy: Policy::allow_all(),
        }
    }

    /// A fixture, only where the kernel really provides `openat2` (Linux). A
    /// sandbox that blocks it (the aarch64 `cross` container) must answer
    /// `Unsupported` and read nothing, which is asserted here; the rest of the
    /// test is skipped with the named gate. x86_64 Linux must have it.
    fn supported() -> Option<Self> {
        let fixture = Self::new();
        let view = fixture.view();
        match view.exists(&wp("."), &cx()) {
            Err(InputError::ContainmentUnsupported) => {
                // Only a non-x86_64 Linux sandbox may answer `Unsupported`;
                // anywhere else (x86_64 Linux, Windows) it is a real failure.
                if !cfg!(target_os = "linux")
                    || (cfg!(target_arch = "x86_64")
                        && std::env::var_os("OMNI_INPUT_VIEW_ALLOW_NO_OPENAT2")
                            .is_none())
                {
                    panic!("this target must not answer Unsupported");
                }
                assert!(matches!(
                    view.read(&wp("a/b/file"), &cx()),
                    Err(InputError::ContainmentUnsupported)
                ));
                assert!(view.evidence().paths.is_empty());
                let _ = omni_test_capabilities::gate(
                    "openat2",
                    false,
                    "blocked by the sandbox",
                );
                None
            }
            Ok(_) => {
                // The probe itself is not part of the test's evidence.
                Some(Self::new())
            }
            other => panic!("unexpected probe result {other:?}"),
        }
    }

    fn view(&self) -> InputView {
        self.view_with(InputLimits::default())
    }

    fn view_with(&self, limits: InputLimits) -> InputView {
        InputView::open(
            &self.root,
            InputViewOptions {
                authorizer: self.policy.clone(),
                limits,
                symlinks: SymlinkPolicy::Reject,
            },
        )
        .unwrap()
    }
}

macro_rules! fixture {
    () => {
        match Fixture::supported() {
            Some(fixture) => fixture,
            None => return,
        }
    };
}

fn wp(path: &str) -> WorkspacePath {
    WorkspacePath::parse(path).unwrap()
}

fn cx() -> Cancellation {
    Cancellation::new()
}

fn digest_of(bytes: &[u8]) -> crate::digest::Digest {
    let mut sink = Blake3Digest::new();
    sink.update(bytes);
    sink.finalize()
}

fn text(read: Read) -> Option<String> {
    match read {
        Read::Found(bytes) => Some(String::from_utf8(bytes.to_vec()).unwrap()),
        Read::Missing => None,
    }
}

fn is_budget(error: &InputError, kind: BudgetKind) -> bool {
    matches!(error, InputError::Budget(found) if *found == kind)
}

#[test]
fn read_returns_the_bytes_and_records_a_file_observation() {
    let fixture = fixture!();
    let view = fixture.view();
    let read = view.read(&wp("a/b/file"), &cx()).unwrap();
    assert_eq!(text(read).as_deref(), Some("inside"));
    let evidence = view.evidence();
    let entry = &evidence.paths[&wp("a/b/file")];
    assert!(!entry.conflict);
    assert!(matches!(
        entry.observations.as_slice(),
        [Observation::File { len: 6, digest, id: Some(_) }]
            if *digest == digest_of(b"inside")
    ));
}

#[test]
fn missing_is_tracked_and_not_a_directory_is_missing() {
    let fixture = fixture!();
    let view = fixture.view();
    assert_eq!(view.read(&wp("a/nope"), &cx()).unwrap(), Read::Missing);
    assert_eq!(
        view.read(&wp("a/b/file/below"), &cx()).unwrap(),
        Read::Missing
    );
    assert_eq!(view.exists(&wp("zzz"), &cx()).unwrap(), Exists::Missing);
    let evidence = view.evidence();
    for path in ["a/nope", "a/b/file/below", "zzz"] {
        assert_eq!(
            evidence.paths[&wp(path)].observations,
            [Observation::Missing],
            "{path}"
        );
    }
}

#[test]
fn a_repeated_read_returns_the_first_captured_bytes() {
    let fixture = fixture!();
    let view = fixture.view();
    let first = view.read(&wp("a/b/file"), &cx()).unwrap();
    fs::write(fixture.root.at("a/b/file"), "changed").unwrap();
    let second = view.read(&wp("a/b/file"), &cx()).unwrap();
    assert_eq!(first, second);
    assert_eq!(text(second).as_deref(), Some("inside"));
}

#[test]
fn exists_reports_the_kind_and_a_directory_is_not_a_regular_read() {
    let fixture = fixture!();
    let view = fixture.view();
    assert_eq!(view.exists(&wp("a/b/file"), &cx()).unwrap(), Exists::File);
    assert_eq!(view.exists(&wp("a/b"), &cx()).unwrap(), Exists::Dir);
    assert_eq!(view.exists(&wp("."), &cx()).unwrap(), Exists::Dir);
    assert!(matches!(
        view.read(&wp("."), &cx()),
        Err(InputError::NotRegular {
            kind: EntryKind::Dir,
            ..
        })
    ));
    assert!(matches!(
        view.read(&wp("a/b"), &cx()),
        Err(InputError::NotRegular {
            kind: EntryKind::Dir,
            ..
        })
    ));
    assert!(matches!(
        view.list(&wp("a/b/file"), &cx()),
        Err(InputError::NotDirectory {
            kind: EntryKind::File,
            ..
        })
    ));
}

#[test]
fn missing_then_created_is_a_conflict_with_both_observations_kept() {
    let fixture = fixture!();
    let view = fixture.view();
    assert_eq!(view.exists(&wp("later"), &cx()).unwrap(), Exists::Missing);
    fs::write(fixture.root.join("later"), "now").unwrap();
    assert_eq!(
        text(view.read(&wp("later"), &cx()).unwrap()).as_deref(),
        Some("now")
    );
    let evidence = view.evidence();
    let entry = &evidence.paths[&wp("later")];
    assert!(entry.conflict);
    assert_eq!(entry.observations.len(), 2);
    assert_eq!(entry.observations[0], Observation::Missing);
}

#[test]
fn exists_then_read_of_the_same_file_is_not_a_conflict() {
    let fixture = fixture!();
    let view = fixture.view();
    view.exists(&wp("a/b/file"), &cx()).unwrap();
    view.read(&wp("a/b/file"), &cx()).unwrap();
    let evidence = view.evidence();
    let entry = &evidence.paths[&wp("a/b/file")];
    assert!(!entry.conflict);
    assert_eq!(entry.observations.len(), 2);
}

#[test]
fn a_refused_first_observation_leaves_no_evidence_entry_behind() {
    let fixture = fixture!();
    let view = fixture.view_with(InputLimits {
        max_observations_per_path: 0,
        max_tracked_inputs: 1,
        ..InputLimits::default()
    });
    assert!(is_budget(
        &view.exists(&wp("first"), &cx()).unwrap_err(),
        BudgetKind::Observations
    ));
    // The refused path did not use up the tracked-input budget.
    assert!(is_budget(
        &view.exists(&wp("second"), &cx()).unwrap_err(),
        BudgetKind::Observations
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn evidence_is_ordered_by_path() {
    let fixture = fixture!();
    let view = fixture.view();
    for path in ["zeta", "a/b/file", "mid"] {
        let _ = view.exists(&wp(path), &cx());
    }
    let paths: Vec<_> = view
        .evidence()
        .paths
        .keys()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(paths, ["a/b/file", "mid", "zeta"]);
}

// Authority: authorizer ∩ containment ∩ limits.

#[test]
fn a_denied_path_is_denied_never_missing_and_is_not_probed() {
    let fixture = fixture!();
    fixture.policy.deny("a/b/file");
    fixture.policy.deny("does/not/exist");
    let view = fixture.view();
    for path in ["a/b/file", "does/not/exist"] {
        assert!(
            matches!(view.read(&wp(path), &cx()), Err(InputError::Denied(_))),
            "{path}"
        );
        assert!(matches!(
            view.exists(&wp(path), &cx()),
            Err(InputError::Denied(_))
        ));
    }
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn an_unevaluable_policy_fails_closed() {
    let fixture = fixture!();
    fixture.policy.fail("a/b/file");
    let view = fixture.view();
    assert!(matches!(
        view.read(&wp("a/b/file"), &cx()),
        Err(InputError::PolicyUnavailable)
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn rejected_names_are_refused_before_the_authorizer_or_the_filesystem() {
    let fixture = fixture!();
    let view = fixture.view();
    for path in ["a/CON", "ab:c", "x/nul.txt", "a/b./c"] {
        assert!(matches!(
            view.read(&wp(path), &cx()),
            Err(InputError::UnsupportedName { .. })
        ));
    }
    assert!(fixture.policy.calls().is_empty());
    assert!(view.evidence().paths.is_empty());
}

#[cfg(unix)]
#[test]
fn containment_rejects_what_the_policy_allows() {
    let fixture = fixture!();
    std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("escape"))
        .unwrap();
    std::os::unix::fs::symlink(
        fixture.outside.join("secret"),
        fixture.root.join("leak"),
    )
    .unwrap();
    let view = fixture.view();
    for path in ["escape/secret", "leak"] {
        assert!(
            matches!(
                view.read(&wp(path), &cx()),
                Err(InputError::Symlink { .. })
            ),
            "{path}"
        );
    }
    assert!(view.evidence().paths.is_empty());
}

#[cfg(windows)]
#[test]
fn containment_rejects_what_the_policy_allows() {
    let fixture = fixture!();
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(fixture.root.join("escape"))
        .arg(&fixture.outside)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let view = fixture.view();
    assert!(matches!(
        view.read(&wp("escape/secret"), &cx()),
        Err(InputError::ReparsePoint { .. })
    ));
    assert!(view.evidence().paths.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_fifo_is_not_regular_and_does_not_block() {
    let fixture = fixture!();
    let fifo = fixture.root.join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let view = fixture.view();
    assert!(matches!(
        view.read(&wp("fifo"), &cx()),
        Err(InputError::NotRegular {
            kind: EntryKind::Other,
            ..
        })
    ));
    assert!(matches!(
        view.exists(&wp("fifo"), &cx()),
        Err(InputError::NotRegular {
            kind: EntryKind::Other,
            ..
        })
    ));
}

// Limits.

#[test]
fn path_shape_limits_are_checked_before_any_io() {
    let fixture = fixture!();
    let view = fixture.view_with(InputLimits {
        max_path_depth: 2,
        max_path_bytes: 12,
        ..InputLimits::default()
    });
    assert!(is_budget(
        &view.read(&wp("x/y/z"), &cx()).unwrap_err(),
        BudgetKind::PathDepth
    ));
    assert!(matches!(
        view.read(&wp("longer-than/twelve"), &cx()),
        Err(InputError::InvalidPath(WorkspacePathError::TooLong {
            max: 12,
            ..
        }))
    ));
    assert!(fixture.policy.calls().is_empty());
}

#[test]
fn an_oversized_file_is_too_large() {
    let fixture = fixture!();
    fs::write(fixture.root.join("big"), "0123456789").unwrap();
    let view = fixture.view_with(InputLimits {
        max_file_bytes: 5,
        ..InputLimits::default()
    });
    assert!(matches!(
        view.read(&wp("big"), &cx()),
        Err(InputError::TooLarge {
            len: 10,
            limit: 5,
            ..
        })
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn a_file_that_grows_during_the_read_is_caught_by_the_cap() {
    let fixture = fixture!();
    let big = fixture.root.join("grow");
    fs::write(&big, vec![b'x'; CHUNK + 10]).unwrap();
    let view = fixture.view_with(InputLimits {
        max_file_bytes: (CHUNK + 20) as u64,
        ..InputLimits::default()
    });
    let mut grown = false;
    let result = view.read_with(&wp("grow"), &cx(), &mut || {
        if !grown {
            grown = true;
            fs::write(&big, vec![b'y'; 4 * CHUNK]).unwrap();
        }
    });
    assert!(
        matches!(result, Err(InputError::TooLarge { .. })),
        "{result:?}"
    );
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn total_bytes_are_charged_before_reading() {
    let fixture = fixture!();
    fs::write(fixture.root.join("one"), "123456").unwrap();
    fs::write(fixture.root.join("two"), "abcdef").unwrap();
    let view = fixture.view_with(InputLimits {
        max_total_bytes: 10,
        ..InputLimits::default()
    });
    view.read(&wp("one"), &cx()).unwrap();
    assert!(is_budget(
        &view.read(&wp("two"), &cx()).unwrap_err(),
        BudgetKind::TotalBytes
    ));
    assert!(!view.evidence().paths.contains_key(&wp("two")));
}

#[test]
fn operations_count_misses_and_failures() {
    let fixture = fixture!();
    let view = fixture.view_with(InputLimits {
        max_operations: 3,
        ..InputLimits::default()
    });
    assert_eq!(view.read(&wp("m1"), &cx()).unwrap(), Read::Missing);
    assert!(view.read(&wp("a/b"), &cx()).is_err());
    assert_eq!(view.exists(&wp("m2"), &cx()).unwrap(), Exists::Missing);
    assert!(is_budget(
        &view.read(&wp("m3"), &cx()).unwrap_err(),
        BudgetKind::Operations
    ));
}

#[test]
fn tracked_inputs_and_observations_per_path_are_bounded() {
    let fixture = fixture!();
    let view = fixture.view_with(InputLimits {
        max_tracked_inputs: 2,
        ..InputLimits::default()
    });
    view.exists(&wp("p1"), &cx()).unwrap();
    view.exists(&wp("p2"), &cx()).unwrap();
    assert!(is_budget(
        &view.exists(&wp("p3"), &cx()).unwrap_err(),
        BudgetKind::Inputs
    ));
    // The same path again still works.
    view.exists(&wp("p1"), &cx()).unwrap();

    let view = fixture.view_with(InputLimits {
        max_observations_per_path: 1,
        ..InputLimits::default()
    });
    assert_eq!(view.exists(&wp("flip"), &cx()).unwrap(), Exists::Missing);
    fs::write(fixture.root.join("flip"), "x").unwrap();
    assert!(is_budget(
        &view.exists(&wp("flip"), &cx()).unwrap_err(),
        BudgetKind::Observations
    ));
}

// Directory listing.

fn listing_fixture() -> Fixture {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.at("d/sub")).unwrap();
    for name in ["b.txt", "a.txt", "c.txt"] {
        fs::write(fixture.root.at("d").join(name), name).unwrap();
    }
    fixture
}

macro_rules! listing {
    () => {
        match Fixture::supported() {
            Some(_) => listing_fixture(),
            None => return,
        }
    };
}

fn names(list: List) -> Vec<(String, EntryKind)> {
    match list {
        List::Found(listing) => listing.entries,
        List::Missing => panic!("missing directory"),
    }
}

fn dir_digest(view: &InputView, path: &str) -> (u32, crate::digest::Digest) {
    match view.evidence().paths[&wp(path)].observations.as_slice() {
        [
            Observation::Dir {
                entry_count,
                membership_digest,
                ..
            },
        ] => (*entry_count, *membership_digest),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn list_is_sorted_single_level_with_kinds() {
    let fixture = listing!();
    let view = fixture.view();
    assert_eq!(
        names(view.list(&wp("d"), &cx()).unwrap()),
        [
            ("a.txt".to_string(), EntryKind::File),
            ("b.txt".to_string(), EntryKind::File),
            ("c.txt".to_string(), EntryKind::File),
            ("sub".to_string(), EntryKind::Dir),
        ]
    );
    assert_eq!(view.list(&wp("nope"), &cx()).unwrap(), List::Missing);
    assert_eq!(names(view.list(&wp("."), &cx()).unwrap()).len(), 2);
}

#[test]
fn a_denied_child_is_omitted_and_does_not_change_the_membership_digest() {
    let denied = listing!();
    denied.policy.deny("d/c.txt");
    let denied_view = denied.view();
    let listed = names(denied_view.list(&wp("d"), &cx()).unwrap());
    assert!(listed.iter().all(|(name, _)| name != "c.txt"));

    let plain = fixture!();
    fs::create_dir_all(plain.root.at("d/sub")).unwrap();
    for name in ["b.txt", "a.txt"] {
        fs::write(plain.root.at("d").join(name), name).unwrap();
    }
    let plain_view = plain.view();
    plain_view.list(&wp("d"), &cx()).unwrap();

    assert_eq!(dir_digest(&denied_view, "d"), dir_digest(&plain_view, "d"));
}

#[test]
fn a_policy_failure_for_one_entry_fails_the_list() {
    let fixture = listing!();
    fixture.policy.fail("d/b.txt");
    let view = fixture.view();
    assert!(matches!(
        view.list(&wp("d"), &cx()),
        Err(InputError::PolicyUnavailable)
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn a_denied_directory_is_denied() {
    let fixture = listing!();
    fixture.policy.deny("d");
    assert!(matches!(
        fixture.view().list(&wp("d"), &cx()),
        Err(InputError::Denied(_))
    ));
}

#[test]
fn raw_scan_threshold_is_the_only_way_denied_entries_can_fail_a_list() {
    // Exactly `max_dir_scan_entries` raw entries (one denied) lists fine; one
    // more denied entry makes the same list fail with DirScan, never with a
    // visible-budget error, and the error carries no counts or names.
    let fixture = fixture!();
    fs::create_dir(fixture.root.join("s")).unwrap();
    for name in ["one", "two", "three"] {
        fs::write(fixture.root.at("s").join(name), name).unwrap();
    }
    fixture.policy.deny("s/three");
    let limits = InputLimits {
        max_dir_scan_entries: 3,
        max_dir_entries: 2,
        ..InputLimits::default()
    };
    let view = fixture.view_with(limits.clone());
    assert_eq!(names(view.list(&wp("s"), &cx()).unwrap()).len(), 2);

    fs::write(fixture.root.at("s").join("four"), "x").unwrap();
    fixture.policy.deny("s/four");
    let view = fixture.view_with(limits);
    let error = view.list(&wp("s"), &cx()).unwrap_err();
    assert!(is_budget(&error, BudgetKind::DirScan), "{error:?}");
}

#[test]
fn visible_budgets_do_not_depend_on_denied_entries() {
    for denied in [0usize, 1, 5] {
        let fixture = fixture!();
        fs::create_dir(fixture.root.join("s")).unwrap();
        for name in ["one", "two"] {
            fs::write(fixture.root.at("s").join(name), name).unwrap();
        }
        for index in 0..denied {
            let name = format!("denied{index}");
            fs::write(fixture.root.at("s").join(&name), "x").unwrap();
            fixture.policy.deny(&format!("s/{name}"));
        }
        let view = fixture.view_with(InputLimits {
            max_dir_entries: 2,
            max_total_dir_entries: 4,
            ..InputLimits::default()
        });
        // Two visible entries fit the per-list limit; the total allows two lists.
        view.list(&wp("s"), &cx()).unwrap();
        view.list(&wp("s"), &cx()).unwrap();
        let error = view.list(&wp("s"), &cx()).unwrap_err();
        assert!(
            is_budget(&error, BudgetKind::TotalDirEntries),
            "{denied}: {error:?}"
        );
    }
}

#[test]
fn dir_entries_limit_stops_the_list() {
    let fixture = listing!();
    let view = fixture.view_with(InputLimits {
        max_dir_entries: 3,
        ..InputLimits::default()
    });
    assert!(is_budget(
        &view.list(&wp("d"), &cx()).unwrap_err(),
        BudgetKind::DirEntries
    ));
    assert!(view.evidence().paths.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_listed_name_failure_says_which_entry_and_why() {
    use omni_diagnostic::Diagnostic as _;
    let fixture = listing!();
    fs::write(fixture.root.at("d").join("ab:c"), "x").unwrap();
    let error = fixture.view().list(&wp("d"), &cx()).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("d/ab:c"), "{message}");
    assert!(message.contains("alternate data stream"), "{message}");
    assert_eq!(error.code(), "input_view.unsupported_name");
}

#[cfg(target_os = "linux")]
#[test]
fn an_unrepresentable_listed_name_is_reported_and_made_printable() {
    use omni_diagnostic::Diagnostic as _;
    // A backslash cannot be part of a workspace path; a newline in a name
    // must not reach the output raw.
    let fixture = listing!();
    fs::write(fixture.root.at("d").join("back\\slash"), "x").unwrap();
    let error = fixture.view().list(&wp("d"), &cx()).unwrap_err();
    assert!(matches!(
        error,
        InputError::UnsupportedName {
            problem: NameProblem::NotRepresentable,
            ..
        }
    ));
    assert!(error.to_string().contains("back\\slash"), "{error}");
    assert_eq!(error.code(), "input_view.unsupported_name");

    let fixture = listing!();
    fs::write(fixture.root.at("d").join("line\nbreak:x"), "x").unwrap();
    let message = fixture
        .view()
        .list(&wp("d"), &cx())
        .unwrap_err()
        .to_string();
    assert!(!message.contains('\n'), "{message:?}");
    assert!(message.contains("line\\nbreak:x"), "{message:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_listed_name_that_is_not_supported_fails_the_list() {
    let fixture = listing!();
    fs::write(fixture.root.at("d").join("ab:c"), "x").unwrap();
    assert!(matches!(
        fixture.view().list(&wp("d"), &cx()),
        Err(InputError::UnsupportedName { .. })
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn a_denied_entry_with_an_unsupported_name_is_omitted_not_an_error() {
    let fixture = listing!();
    fs::write(fixture.root.at("d").join("ab:c"), "x").unwrap();
    fixture.policy.deny("d/ab:c");
    let listed = names(fixture.view().list(&wp("d"), &cx()).unwrap());
    assert!(listed.iter().all(|(name, _)| name != "ab:c"));
    assert_eq!(listed.len(), 4);
}

#[test]
fn membership_digest_changes_when_an_entry_is_added() {
    let fixture = listing!();
    let view = fixture.view();
    view.list(&wp("d"), &cx()).unwrap();
    let first = dir_digest(&view, "d");
    fs::write(fixture.root.at("d").join("new"), "x").unwrap();
    view.list(&wp("d"), &cx()).unwrap();
    let evidence = view.evidence();
    let entry = &evidence.paths[&wp("d")];
    assert!(entry.conflict);
    assert_eq!(entry.observations.len(), 2);
    assert_ne!(first.0, 0);
}

// Cancellation.

#[test]
fn a_cancelled_call_records_nothing() {
    let fixture = fixture!();
    let view = fixture.view();
    let cancelled = cx();
    cancelled.cancel();
    assert!(matches!(
        view.read(&wp("a/b/file"), &cancelled),
        Err(InputError::Cancelled)
    ));
    assert!(matches!(
        view.list(&wp("a"), &cancelled),
        Err(InputError::Cancelled)
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn cancelling_mid_read_exposes_no_bytes_and_records_nothing() {
    let fixture = fixture!();
    fs::write(fixture.root.join("large"), vec![b'z'; 3 * CHUNK]).unwrap();
    let view = fixture.view();
    let cancel = cx();
    let result = view.read_with(&wp("large"), &cancel, &mut || cancel.cancel());
    assert!(matches!(result, Err(InputError::Cancelled)), "{result:?}");
    assert!(view.evidence().paths.is_empty());
    // Nothing was captured: a later read returns the whole file.
    let read = view.read(&wp("large"), &cx()).unwrap();
    assert!(matches!(read, Read::Found(bytes) if bytes.len() == 3 * CHUNK));
}

// Case spelling (D8) on a case-insensitive filesystem: Windows reads the
// on-disk names back from the handle.

#[cfg(windows)]
#[test]
fn a_wrong_case_request_is_a_case_mismatch_and_records_nothing() {
    let fixture = fixture!();
    fs::write(fixture.root.join("Readme.md"), "x").unwrap();
    let view = fixture.view();
    assert!(matches!(
        view.read(&wp("readme.md"), &cx()),
        Err(InputError::CaseMismatch { .. })
    ));
    assert!(matches!(
        view.read(&wp("A/b/file"), &cx()),
        Err(InputError::CaseMismatch { .. })
    ));
    assert!(view.evidence().paths.is_empty());
    assert_eq!(
        text(view.read(&wp("Readme.md"), &cx()).unwrap()).as_deref(),
        Some("x")
    );
}

#[test]
fn defaults_match_the_recorded_decision() {
    let limits = InputLimits::default();
    assert_eq!(limits.max_observations_per_path, 16);
    assert_eq!(limits.max_file_bytes, 8 * 1024 * 1024);
}

#[test]
fn overlapping_reads_keep_the_first_capture() {
    let fixture = fixture!();
    let view = fixture.view();
    let path = wp("a/b/file");
    let metadata = fs::metadata(fixture.root.at("a/b/file")).unwrap();
    let first: Arc<[u8]> = Arc::from(&b"first"[..]);
    let second: Arc<[u8]> = Arc::from(&b"second"[..]);
    let kept = view
        .record_file(
            &cx(),
            &path,
            first.clone(),
            digest_of(b"first"),
            &metadata,
        )
        .unwrap();
    assert_eq!(kept, first);
    let kept = view
        .record_file(&cx(), &path, second, digest_of(b"second"), &metadata)
        .unwrap();
    assert_eq!(kept, first, "the later capture must not replace the first");
    assert_eq!(view.evidence().paths[&path].observations.len(), 1);
}

#[test]
fn an_authorizer_that_cancels_stops_the_call() {
    struct Cancelling(Cancellation);
    impl InputAuthorizer for Cancelling {
        fn authorize_read(
            &self,
            _: &WorkspacePath,
        ) -> Result<AuthDecision, AuthError> {
            self.0.cancel();
            Ok(AuthDecision::Allow)
        }
    }
    let fixture = fixture!();
    let cancel = cx();
    let view = InputView::open(
        &fixture.root,
        InputViewOptions {
            authorizer: Arc::new(Cancelling(cancel.clone())),
            limits: InputLimits::default(),
            symlinks: SymlinkPolicy::Reject,
        },
    )
    .unwrap();
    assert!(matches!(
        view.exists(&wp("a/b/file"), &cancel),
        Err(InputError::Cancelled)
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn growth_during_the_read_is_charged_before_buffering() {
    let fixture = fixture!();
    let big = fixture.root.join("grow");
    fs::write(&big, vec![b'x'; CHUNK + 10]).unwrap();
    let view = fixture.view_with(InputLimits {
        max_total_bytes: (CHUNK + 10) as u64 + 100,
        ..InputLimits::default()
    });
    let mut chunks = 0usize;
    let result = view.read_with(&wp("grow"), &cx(), &mut || {
        chunks += 1;
        if chunks == 1 {
            fs::write(&big, vec![b'y'; 4 * CHUNK]).unwrap();
        }
    });
    // Refused when the first grown chunk was seen, before any more was buffered.
    assert_eq!(chunks, 1);
    assert!(
        matches!(result, Err(InputError::Budget(BudgetKind::TotalBytes))),
        "{result:?}"
    );
    assert!(view.evidence().paths.is_empty());
}

// F10.2c: symlinks are rejected; following is not implemented.

#[test]
fn following_symlinks_is_unsupported_and_follows_nothing() {
    let fixture = Fixture::new();
    for max_hops in [0u8, 1, 8] {
        let error = InputView::open(
            &fixture.root,
            InputViewOptions {
                authorizer: fixture.policy.clone(),
                limits: InputLimits::default(),
                symlinks: SymlinkPolicy::FollowInRoot { max_hops },
            },
        )
        .err()
        .expect("FollowInRoot must not open a view");
        assert!(matches!(error, InputError::SymlinkPolicyUnsupported));
    }
    // Refused before the root is touched, even a root that does not exist.
    let missing = fixture.root.join("does-not-exist");
    let error = InputView::open(
        &missing,
        InputViewOptions {
            authorizer: fixture.policy.clone(),
            limits: InputLimits::default(),
            symlinks: SymlinkPolicy::FollowInRoot { max_hops: 1 },
        },
    )
    .err()
    .expect("FollowInRoot must not open a view");
    assert!(matches!(error, InputError::SymlinkPolicyUnsupported));
    // Not even the authorizer was consulted: nothing was read.
    assert!(fixture.policy.calls().is_empty());
    assert_eq!(SymlinkPolicy::default(), SymlinkPolicy::Reject);
}

#[test]
fn an_ancestor_cancelled_after_admission_records_nothing() {
    // Cancelling from the byte hook (after admission, before the evidence is
    // published) must not leave an observation behind.
    let fixture = fixture!();
    let view = fixture.view();
    let cancel = cx();
    let result =
        view.read_with(&wp("a/b/file"), &cancel, &mut || cancel.cancel());
    assert!(matches!(result, Err(InputError::Cancelled)), "{result:?}");
    assert!(view.evidence().paths.is_empty());
    // A cached read after cancellation is refused too.
    view.read(&wp("a/b/file"), &cx()).unwrap();
    let cancelled = cx();
    cancelled.cancel();
    assert!(matches!(
        view.read(&wp("a/b/file"), &cancelled),
        Err(InputError::Cancelled)
    ));
}

#[test]
fn evidence_is_not_published_for_a_cancelled_call() {
    let fixture = fixture!();
    let view = fixture.view();
    let path = wp("a/b/file");
    let metadata = fs::metadata(fixture.root.at("a/b/file")).unwrap();
    let cancelled = cx();
    cancelled.cancel();
    assert!(matches!(
        view.record(&cancelled, &path, Observation::Missing),
        Err(InputError::Cancelled)
    ));
    assert!(matches!(
        view.record_file(
            &cancelled,
            &path,
            Arc::from(&b"inside"[..]),
            digest_of(b"inside"),
            &metadata
        ),
        Err(InputError::Cancelled)
    ));
    assert!(view.evidence().paths.is_empty());
}

#[cfg(unix)]
mod reject_matrix {
    use std::os::unix::fs::symlink;

    use super::*;

    fn assert_symlink(view: &InputView, path: &str) {
        for result in [
            view.read(&wp(path), &cx()).map(|_| ()),
            view.exists(&wp(path), &cx()).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(InputError::Symlink { .. })),
                "{path}: {result:?}"
            );
        }
    }

    #[test]
    fn every_link_shape_is_a_containment_error_and_reads_nothing() {
        let fixture = fixture!();
        let root = &fixture.root;
        symlink(fixture.outside.join("secret"), root.join("absolute")).unwrap();
        symlink("../outside/secret", root.join("dotdot")).unwrap();
        symlink("loop", root.join("loop")).unwrap();
        symlink("gone", root.join("dangling")).unwrap();
        symlink(root.at("a/b/file"), root.join("inside_abs")).unwrap();
        symlink("a/b/file", root.join("inside_rel")).unwrap();
        symlink("b", root.at("a/dir_link")).unwrap();
        let view = fixture.view();
        for path in [
            "absolute",
            "dotdot",
            "loop",
            "dangling",
            "inside_abs",
            "inside_rel",
            "a/dir_link/file",
            "a/dir_link",
        ] {
            assert_symlink(&view, path);
        }
        assert!(view.evidence().paths.is_empty());
    }

    #[test]
    fn a_retargeted_link_never_yields_bytes() {
        let fixture = fixture!();
        let link = fixture.root.join("moving");
        symlink(fixture.outside.join("secret"), &link).unwrap();
        let view = fixture.view();
        assert_symlink(&view, "moving");
        fs::remove_file(&link).unwrap();
        symlink(fixture.root.at("a/b/file"), &link).unwrap();
        assert_symlink(&view, "moving");
        fs::remove_file(&link).unwrap();
        // Once it is a real file the same path reads normally.
        fs::write(&link, "now a file").unwrap();
        assert_eq!(
            text(view.read(&wp("moving"), &cx()).unwrap()).as_deref(),
            Some("now a file")
        );
    }

    #[test]
    fn a_listed_symlink_is_reported_as_a_symlink_not_followed() {
        let fixture = fixture!();
        symlink(&fixture.outside, fixture.root.join("link")).unwrap();
        let view = fixture.view();
        let listed = names(view.list(&wp("."), &cx()).unwrap());
        assert!(listed.contains(&("link".to_string(), EntryKind::Symlink)));
        assert!(matches!(
            view.list(&wp("link"), &cx()),
            Err(InputError::Symlink { .. })
        ));
    }
}

#[cfg(windows)]
mod reject_matrix_windows {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    use omni_test_capabilities::Capability;

    use super::*;

    fn junction(link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    fn assert_reparse(view: &InputView, path: &str) {
        for result in [
            view.read(&wp(path), &cx()).map(|_| ()),
            view.exists(&wp(path), &cx()).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(InputError::ReparsePoint { .. })),
                "{path}: {result:?}"
            );
        }
    }

    // Junctions need no privilege, so these never skip.
    #[test]
    fn junctions_are_rejected_and_a_retargeted_one_never_yields_bytes() {
        let fixture = fixture!();
        let link = fixture.root.join("junction");
        junction(&link, &fixture.outside);
        let view = fixture.view();
        assert_reparse(&view, "junction");
        assert_reparse(&view, "junction/secret");
        fs::remove_dir(&link).unwrap();
        junction(&link, &fixture.root.at("a/b"));
        assert_reparse(&view, "junction");
        assert_reparse(&view, "junction/file");
        assert!(matches!(
            view.list(&wp("junction"), &cx()),
            Err(InputError::ReparsePoint { .. })
        ));
        assert!(view.evidence().paths.is_empty());
    }

    #[test]
    fn symlink_shapes_are_rejected() {
        let fixture = fixture!();
        if !omni_test_capabilities::require(Capability::Symlink, &fixture.root)
        {
            return;
        }
        let root = &fixture.root;
        symlink_file(fixture.outside.join("secret"), root.join("absolute"))
            .unwrap();
        symlink_file("..\\outside\\secret", root.join("dotdot")).unwrap();
        symlink_file("gone", root.join("dangling")).unwrap();
        symlink_file("loop", root.join("loop")).unwrap();
        symlink_dir("b", root.at("a/dir_link")).unwrap();
        let view = fixture.view();
        for path in [
            "absolute",
            "dotdot",
            "dangling",
            "loop",
            "a/dir_link",
            "a/dir_link/file",
        ] {
            assert_reparse(&view, path);
        }
        assert!(view.evidence().paths.is_empty());
    }
}

// F10.3: revalidation and reject-on-change.

mod revalidation {
    use super::*;

    fn changed(view: &InputView) -> Vec<(String, ChangeKind)> {
        match view.revalidate(&cx()).unwrap() {
            Revalidation::Unchanged => Vec::new(),
            Revalidation::Changed { changes, remainder } => {
                assert_eq!(remainder, 0);
                changes
                    .into_iter()
                    .map(|change| (change.path.to_string(), change.kind))
                    .collect()
            }
        }
    }

    fn track_file(view: &InputView) {
        assert!(matches!(
            view.read(&wp("a/b/file"), &cx()).unwrap(),
            Read::Found(_)
        ));
    }

    #[test]
    fn nothing_changed_is_unchanged_and_repeatable() {
        let fixture = listing!();
        let view = fixture.view();
        track_file(&view);
        view.read(&wp("missing"), &cx()).unwrap();
        view.exists(&wp("a"), &cx()).unwrap();
        view.list(&wp("d"), &cx()).unwrap();
        let before = view.evidence();
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
        // Read-only: the evidence is untouched, and so is the first capture.
        assert_eq!(view.evidence(), before);
        assert!(view.revalidate(&cx()).unwrap().reject_on_change().is_ok());
    }

    #[test]
    fn a_same_size_same_mtime_edit_is_modified() {
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        let path = fixture.root.at("a/b/file");
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, "insidf").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(after.len(), 6);
        assert_eq!(after.modified().unwrap(), mtime);
        assert_eq!(
            changed(&view),
            [("a/b/file".to_string(), ChangeKind::Modified)]
        );
    }

    #[test]
    fn an_atomic_replace_with_identical_bytes_is_not_a_change() {
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        let path = fixture.root.at("a/b/file");
        let temp = fixture.root.at("a/b/file.tmp");
        fs::write(&temp, "inside").unwrap();
        fs::rename(&temp, &path).unwrap();
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
    }

    #[test]
    fn created_after_a_missing_read_and_deleted_and_kind_changes() {
        let fixture = fixture!();
        fs::write(fixture.root.join("gone"), "x").unwrap();
        fs::write(fixture.root.join("swap"), "x").unwrap();
        let view = fixture.view();
        view.read(&wp("later"), &cx()).unwrap();
        view.read(&wp("gone"), &cx()).unwrap();
        view.read(&wp("swap"), &cx()).unwrap();
        view.exists(&wp("a"), &cx()).unwrap();
        fs::write(fixture.root.join("later"), "now").unwrap();
        fs::remove_file(fixture.root.join("gone")).unwrap();
        fs::remove_file(fixture.root.join("swap")).unwrap();
        fs::create_dir(fixture.root.join("swap")).unwrap();
        fs::remove_dir_all(fixture.root.join("a")).unwrap();
        fs::write(fixture.root.join("a"), "now a file").unwrap();
        assert_eq!(
            changed(&view),
            [
                ("a".to_string(), ChangeKind::KindChanged),
                ("gone".to_string(), ChangeKind::Deleted),
                ("later".to_string(), ChangeKind::Created),
                ("swap".to_string(), ChangeKind::KindChanged),
            ]
        );
    }

    #[test]
    fn a_parent_that_became_a_file_counts_as_missing() {
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        fs::remove_dir_all(fixture.root.at("a/b")).unwrap();
        fs::write(fixture.root.at("a/b"), "a file now").unwrap();
        assert_eq!(
            changed(&view),
            [("a/b/file".to_string(), ChangeKind::Deleted)]
        );
    }

    #[test]
    fn membership_changes_are_detected() {
        let fixture = listing!();
        let view = fixture.view();
        view.list(&wp("d"), &cx()).unwrap();
        // Add, remove and rename each change the membership.
        for step in 0..3 {
            let dir = fixture.root.at("d");
            match step {
                0 => fs::write(dir.join("new"), "x").unwrap(),
                1 => fs::remove_file(dir.join("new")).unwrap(),
                _ => fs::rename(dir.join("a.txt"), dir.join("z.txt")).unwrap(),
            }
            let result = changed(&view);
            if step == 1 {
                assert!(result.is_empty(), "back to the original membership");
            } else {
                assert_eq!(
                    result,
                    [("d".to_string(), ChangeKind::MembershipChanged)],
                    "step {step}"
                );
            }
        }
    }

    #[test]
    fn a_denied_entry_appearing_is_invisible() {
        let fixture = listing!();
        fixture.policy.deny("d/secret.txt");
        let view = fixture.view();
        view.list(&wp("d"), &cx()).unwrap();
        fs::write(fixture.root.at("d").join("secret.txt"), "x").unwrap();
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
    }

    #[test]
    fn conflicting_observations_are_a_change_without_a_re_read() {
        let fixture = fixture!();
        let view = fixture.view();
        view.exists(&wp("later"), &cx()).unwrap();
        fs::write(fixture.root.join("later"), "now").unwrap();
        view.read(&wp("later"), &cx()).unwrap();
        // The final state is stable and equals the last observation, yet the
        // pass saw Missing then a file.
        assert_eq!(
            changed(&view),
            [("later".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn the_strongest_earlier_observation_is_the_baseline() {
        // exists() records only the kind; the later read/list records content.
        // The two are compatible, so no conflict is flagged: the pass must
        // compare against the content, not the first (kind-only) observation.
        let fixture = listing!();
        let view = fixture.view();
        view.exists(&wp("a/b/file"), &cx()).unwrap();
        view.read(&wp("a/b/file"), &cx()).unwrap();
        view.exists(&wp("d"), &cx()).unwrap();
        view.list(&wp("d"), &cx()).unwrap();
        assert!(!view.evidence().paths[&wp("a/b/file")].conflict);
        assert!(!view.evidence().paths[&wp("d")].conflict);
        fs::write(fixture.root.at("a/b/file"), "insidf").unwrap();
        fs::write(fixture.root.at("d").join("new"), "x").unwrap();
        assert_eq!(
            changed(&view),
            [
                ("a/b/file".to_string(), ChangeKind::Modified),
                ("d".to_string(), ChangeKind::MembershipChanged),
            ]
        );
    }

    #[test]
    fn a_listed_directory_that_vanishes_or_becomes_a_file_is_a_change() {
        let fixture = listing!();
        fs::create_dir(fixture.root.join("e")).unwrap();
        let view = fixture.view();
        view.list(&wp("d"), &cx()).unwrap();
        view.list(&wp("e"), &cx()).unwrap();
        fs::remove_dir_all(fixture.root.join("d")).unwrap();
        fs::remove_dir(fixture.root.join("e")).unwrap();
        fs::write(fixture.root.join("e"), "a file").unwrap();
        assert_eq!(
            changed(&view),
            [
                ("d".to_string(), ChangeKind::Deleted),
                ("e".to_string(), ChangeKind::KindChanged),
            ]
        );
    }

    #[test]
    fn a_pass_cancelled_before_it_starts_is_cancelled_even_when_empty() {
        let fixture = fixture!();
        let view = fixture.view();
        let cancelled = cx();
        cancelled.cancel();
        assert!(matches!(
            view.revalidate(&cancelled),
            Err(InputError::Cancelled)
        ));
    }

    #[test]
    fn a_directory_that_outgrew_its_limit_is_an_error_not_a_change() {
        let fixture = listing!();
        let view = fixture.view_with(InputLimits {
            max_dir_entries: 5,
            ..InputLimits::default()
        });
        view.list(&wp("d"), &cx()).unwrap();
        for name in ["n1", "n2", "n3"] {
            fs::write(fixture.root.at("d").join(name), "x").unwrap();
        }
        assert!(matches!(
            view.revalidate(&cx()),
            Err(InputError::Budget(BudgetKind::DirEntries))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_directory_replaced_by_a_junction_is_a_containment_error_not_a_change()
    {
        let fixture = fixture!();
        let view = fixture.view();
        view.exists(&wp("a/b"), &cx()).unwrap();
        fs::remove_dir_all(fixture.root.at("a/b")).unwrap();
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(fixture.root.at("a/b"))
            .arg(&fixture.outside)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(matches!(
            view.revalidate(&cx()),
            Err(InputError::ReparsePoint { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_differently_spelled_entry_appearing_for_a_missing_name_is_created() {
        let fixture = fixture!();
        let view = fixture.view();
        assert_eq!(view.read(&wp("Readme.md"), &cx()).unwrap(), Read::Missing);
        fs::write(fixture.root.join("README.MD"), "x").unwrap();
        assert_eq!(
            changed(&view),
            [("Readme.md".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn present_only_tracks_the_kind() {
        let fixture = fixture!();
        let view = fixture.view();
        view.exists(&wp("a/b/file"), &cx()).unwrap();
        fs::write(fixture.root.at("a/b/file"), "edited").unwrap();
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
        fs::remove_file(fixture.root.at("a/b/file")).unwrap();
        assert_eq!(
            changed(&view),
            [("a/b/file".to_string(), ChangeKind::Deleted)]
        );
    }

    #[test]
    fn output_is_bounded_with_a_remainder_and_deterministic() {
        let fixture = fixture!();
        let view = fixture.view_with(InputLimits {
            max_reported_changes: 2,
            ..InputLimits::default()
        });
        for name in ["c1", "a1", "d1", "b1"] {
            view.read(&wp(name), &cx()).unwrap();
            fs::write(fixture.root.join(name), name).unwrap();
        }
        let expected = Revalidation::Changed {
            changes: vec![
                Change {
                    path: wp("a1"),
                    kind: ChangeKind::Created,
                },
                Change {
                    path: wp("b1"),
                    kind: ChangeKind::Created,
                },
            ],
            remainder: 2,
        };
        assert_eq!(view.revalidate(&cx()).unwrap(), expected);
        assert_eq!(view.revalidate(&cx()).unwrap(), expected);
        let rejected = expected.reject_on_change().unwrap_err();
        assert_eq!((rejected.changes.len(), rejected.remainder), (2, 2));
    }

    #[test]
    fn denial_containment_io_and_budget_failures_are_errors_not_changes() {
        // Denied during the pass.
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        fixture.policy.deny("a/b/file");
        assert!(matches!(view.revalidate(&cx()), Err(InputError::Denied(_))));

        // Unevaluable policy.
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        fixture.policy.fail("a/b/file");
        assert!(matches!(
            view.revalidate(&cx()),
            Err(InputError::PolicyUnavailable)
        ));

        // A limit hit during the pass (the file grew past the cap).
        let fixture = fixture!();
        let view = fixture.view_with(InputLimits {
            max_file_bytes: 10,
            ..InputLimits::default()
        });
        track_file(&view);
        fs::write(fixture.root.at("a/b/file"), vec![b'x'; 50]).unwrap();
        assert!(matches!(
            view.revalidate(&cx()),
            Err(InputError::TooLarge { .. })
        ));
    }

    #[test]
    fn the_pass_has_its_own_budgets() {
        // Tracking used every operation; the pass still has a full set.
        let fixture = fixture!();
        fs::write(fixture.root.join("second"), "x").unwrap();
        let view = fixture.view_with(InputLimits {
            max_operations: 2,
            max_total_bytes: 7,
            ..InputLimits::default()
        });
        track_file(&view);
        view.read(&wp("second"), &cx()).unwrap();
        assert!(matches!(
            view.read(&wp("third"), &cx()),
            Err(InputError::Budget(BudgetKind::Operations))
        ));
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_replaced_by_a_symlink_is_a_containment_error_not_a_change() {
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        let path = fixture.root.at("a/b/file");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(fixture.outside.join("secret"), &path)
            .unwrap();
        assert!(matches!(
            view.revalidate(&cx()),
            Err(InputError::Symlink { .. })
        ));
    }

    #[test]
    fn cancellation_leaves_the_state_untouched() {
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        let before = view.evidence();
        let cancelled = cx();
        cancelled.cancel();
        assert!(matches!(
            view.revalidate(&cancelled),
            Err(InputError::Cancelled)
        ));
        assert_eq!(view.evidence(), before);
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
    }

    #[test]
    fn an_edit_after_the_last_re_observation_is_not_asserted_as_detected() {
        // The honest guarantee: only changes before the last re-observation
        // are found. This documents the boundary and asserts nothing about
        // the later edit.
        let fixture = fixture!();
        let view = fixture.view();
        track_file(&view);
        assert_eq!(view.revalidate(&cx()).unwrap(), Revalidation::Unchanged);
        fs::write(fixture.root.at("a/b/file"), "later!").unwrap();
        // Nothing is asserted here: the pass already returned.
    }

    #[cfg(windows)]
    #[test]
    fn a_case_only_rename_of_a_tracked_path_is_deleted() {
        let fixture = fixture!();
        fs::write(fixture.root.join("Readme.md"), "x").unwrap();
        let view = fixture.view();
        view.read(&wp("Readme.md"), &cx()).unwrap();
        fs::rename(
            fixture.root.join("Readme.md"),
            fixture.root.join("README.MD"),
        )
        .unwrap();
        assert_eq!(
            changed(&view),
            [("Readme.md".to_string(), ChangeKind::Deleted)]
        );
    }
}
