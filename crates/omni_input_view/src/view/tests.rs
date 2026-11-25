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

    fn view(&self) -> InputView {
        self.view_with(InputLimits::default())
    }

    fn view_with(&self, limits: InputLimits) -> InputView {
        InputView::open(
            &self.root,
            InputViewOptions {
                authorizer: self.policy.clone(),
                limits,
            },
        )
        .unwrap()
    }
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
    let view = fixture.view();
    let first = view.read(&wp("a/b/file"), &cx()).unwrap();
    fs::write(fixture.root.at("a/b/file"), "changed").unwrap();
    let second = view.read(&wp("a/b/file"), &cx()).unwrap();
    assert_eq!(first, second);
    assert_eq!(text(second).as_deref(), Some("inside"));
}

#[test]
fn exists_reports_the_kind_and_a_directory_is_not_a_regular_read() {
    let fixture = Fixture::new();
    let view = fixture.view();
    assert_eq!(view.exists(&wp("a/b/file"), &cx()).unwrap(), Exists::File);
    assert_eq!(view.exists(&wp("a/b"), &cx()).unwrap(), Exists::Dir);
    assert_eq!(view.exists(&wp("."), &cx()).unwrap(), Exists::Dir);
    assert!(matches!(
        view.read(&wp("."), &cx()),
        Err(InputError::NotRegular(EntryKind::Dir))
    ));
    assert!(matches!(
        view.read(&wp("a/b"), &cx()),
        Err(InputError::NotRegular(EntryKind::Dir))
    ));
    assert!(matches!(
        view.list(&wp("a/b/file"), &cx()),
        Err(InputError::NotDirectory(EntryKind::File))
    ));
}

#[test]
fn missing_then_created_is_a_conflict_with_both_observations_kept() {
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
    let view = fixture.view();
    view.exists(&wp("a/b/file"), &cx()).unwrap();
    view.read(&wp("a/b/file"), &cx()).unwrap();
    let evidence = view.evidence();
    let entry = &evidence.paths[&wp("a/b/file")];
    assert!(!entry.conflict);
    assert_eq!(entry.observations.len(), 2);
}

#[test]
fn evidence_is_ordered_by_path() {
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
    let view = fixture.view();
    for path in ["a/CON", "ab:c", "x/nul.txt", "a/b./c"] {
        assert!(matches!(
            view.read(&wp(path), &cx()),
            Err(InputError::Containment(
                ContainmentViolation::NameUnsupported
            ))
        ));
    }
    assert!(fixture.policy.calls().is_empty());
    assert!(view.evidence().paths.is_empty());
}

#[cfg(unix)]
#[test]
fn containment_rejects_what_the_policy_allows() {
    let fixture = Fixture::new();
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
                Err(InputError::Containment(ContainmentViolation::Symlink))
            ),
            "{path}"
        );
    }
    assert!(view.evidence().paths.is_empty());
}

#[cfg(windows)]
#[test]
fn containment_rejects_what_the_policy_allows() {
    let fixture = Fixture::new();
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
        Err(InputError::Containment(ContainmentViolation::ReparsePoint))
    ));
    assert!(view.evidence().paths.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_fifo_is_not_regular_and_does_not_block() {
    let fixture = Fixture::new();
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
        Err(InputError::NotRegular(EntryKind::Other))
    ));
    assert!(matches!(
        view.exists(&wp("fifo"), &cx()),
        Err(InputError::NotRegular(EntryKind::Other))
    ));
}

// Limits.

#[test]
fn path_shape_limits_are_checked_before_any_io() {
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
    fs::write(fixture.root.join("big"), "0123456789").unwrap();
    let view = fixture.view_with(InputLimits {
        max_file_bytes: 5,
        ..InputLimits::default()
    });
    assert!(matches!(
        view.read(&wp("big"), &cx()),
        Err(InputError::TooLarge { len: 10, limit: 5 })
    ));
    assert!(view.evidence().paths.is_empty());
}

#[test]
fn a_file_that_grows_during_the_read_is_caught_by_the_cap() {
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = listing_fixture();
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
    let denied = listing_fixture();
    denied.policy.deny("d/c.txt");
    let denied_view = denied.view();
    let listed = names(denied_view.list(&wp("d"), &cx()).unwrap());
    assert!(listed.iter().all(|(name, _)| name != "c.txt"));

    let plain = Fixture::new();
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
    let fixture = listing_fixture();
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
    let fixture = listing_fixture();
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
    let fixture = Fixture::new();
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
        let fixture = Fixture::new();
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
    let fixture = listing_fixture();
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
fn a_listed_name_that_is_not_supported_fails_the_list() {
    let fixture = listing_fixture();
    fs::write(fixture.root.at("d").join("ab:c"), "x").unwrap();
    assert!(matches!(
        fixture.view().list(&wp("d"), &cx()),
        Err(InputError::Containment(
            ContainmentViolation::NameUnsupported
        ))
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn a_denied_entry_with_an_unsupported_name_is_omitted_not_an_error() {
    let fixture = listing_fixture();
    fs::write(fixture.root.at("d").join("ab:c"), "x").unwrap();
    fixture.policy.deny("d/ab:c");
    let listed = names(fixture.view().list(&wp("d"), &cx()).unwrap());
    assert!(listed.iter().all(|(name, _)| name != "ab:c"));
    assert_eq!(listed.len(), 4);
}

#[test]
fn membership_digest_changes_when_an_entry_is_added() {
    let fixture = listing_fixture();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
    let fixture = Fixture::new();
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
