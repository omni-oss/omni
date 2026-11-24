mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::*;
use omni_migration_proof::core::*;
use omni_migration_proof::fs::{DurableFs, FAILPOINT_ENV, Faults};
use omni_migration_proof::index::*;
use omni_migration_proof::lockfile::*;
use tempfile::tempdir;

fn child(scenario: &str, root: &Path, failpoints: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "child_entry", "--nocapture", "--test-threads=1"])
        .env("OMNI_MIG_SCENARIO", scenario)
        .env("OMNI_MIG_ROOT", root)
        .env(FAILPOINT_ENV, failpoints)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// Entry point re-executed in a subprocess; a no-op in a normal run.
#[test]
fn child_entry() {
    let Ok(scenario) = std::env::var("OMNI_MIG_SCENARIO") else {
        return;
    };
    let root =
        std::path::PathBuf::from(std::env::var("OMNI_MIG_ROOT").unwrap());
    let ok = match scenario.as_str() {
        "lockfile_up" => {
            let s = LockfileStore::open(
                &root,
                Faults::from_env(),
                PinsCatalog::two(),
            );
            let plan = s
                .plan(PINS_V2, Direction::Upgrade, Policy::default())
                .unwrap();
            matches!(execute(&s, &plan, || false), Ok(Run::Finished(_)))
        }
        "index_drop" => {
            let s = IndexStore::open(&root, Faults::from_env());
            let plan = s
                .plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE)
                .unwrap();
            matches!(execute(&s, &plan, || false), Ok(Run::Finished(_)))
        }
        other => panic!("unknown scenario {other}"),
    };
    std::process::exit(if ok { 0 } else { 3 });
}

fn lockfile(root: &Path) -> LockfileStore {
    v1_store(root, PinsCatalog::two())
}

#[test]
fn lockfile_killed_after_intent_and_after_replace() {
    for (point, expect_new) in
        [("doc.rename=exit", false), ("doc.parent_sync=exit", true)]
    {
        let dir = tempdir().unwrap();
        let s = lockfile(dir.path());
        let status = child("lockfile_up", dir.path(), point).status().unwrap();
        assert_eq!(status.code(), Some(86), "{point}");
        assert_eq!(s.state().unwrap(), StoreState::RecoveryRequired);
        let want = if expect_new {
            Recovered::New
        } else {
            Recovered::Old
        };
        assert_eq!(s.recover_under_guard(), Ok(want), "{point}");
        assert_eq!(
            s.state().unwrap(),
            StoreState::Ready(if expect_new { PINS_V2 } else { PINS_V1 })
        );
    }
}

#[test]
fn index_killed_after_first_shard_removal() {
    let dir = tempdir().unwrap();
    let s = IndexStore::create(
        dir.path(),
        Faults::new(),
        5,
        WITH_INDEX,
        &["bb", "a", "ccc"],
    )
    .unwrap();
    let original = s.shard_bytes().unwrap();
    let status = child("index_drop", dir.path(), "shard.removed=exit")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86));
    assert_eq!(s.read_marker_version(), Ok(WITH_INDEX));
    assert_eq!(s.state().unwrap(), StoreState::RecoveryRequired);
    assert_eq!(s.recover_under_guard(), Ok(IndexRecovered::RolledBack));
    assert_eq!(s.shard_bytes().unwrap(), original);
}

#[test]
fn a_live_writer_blocks_peers_and_the_survivor_rejects_stale_plans() {
    let dir = tempdir().unwrap();
    let s = lockfile(dir.path());
    let stale = s
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    let ready = dir.path().join("ready");
    let release = dir.path().join("release");
    let fp =
        format!("doc.rename=wait:{}:{}", ready.display(), release.display());
    let mut first = child("lockfile_up", dir.path(), &fp).spawn().unwrap();
    let start = Instant::now();
    while !ready.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "child never reached the barrier"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // The first writer is live and holds an intent: a peer cannot take the
    // guard, and cannot observe a clean state.
    let peer = DurableFs::new(Faults::new());
    assert_eq!(
        peer.try_lock_exclusive(&s.guard_path()).err(),
        Some(Error::GuardBusy)
    );
    assert_eq!(s.state().unwrap(), StoreState::RecoveryRequired);
    std::fs::write(&release, b"go").unwrap();
    assert!(first.wait().unwrap().success());
    assert!(peer.try_lock_exclusive(&s.guard_path()).is_ok());
    // The second writer re-observes under the guard and rejects its stale plan.
    assert!(matches!(
        execute(&s, &stale, || false),
        Err(Error::StalePlan)
    ));
    assert_eq!(s.state().unwrap(), StoreState::Ready(PINS_V2));
}
