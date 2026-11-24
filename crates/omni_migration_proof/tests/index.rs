mod common;

use common::*;
use omni_migration_proof::core::*;
use omni_migration_proof::fs::{Action, Faults};
use omni_migration_proof::index::*;
use tempfile::tempdir;

fn idx(root: &std::path::Path, v: Version) -> IndexStore {
    IndexStore::create(root, Faults::new(), 5, v, &["bb", "a", "ccc"]).unwrap()
}

fn drop_plan(s: &IndexStore) -> Plan<IndexStep> {
    s.plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE)
        .unwrap()
}

#[test]
fn round_trip_reconstructs_shards_from_records() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let original = s.shard_bytes().unwrap();
    assert_eq!(original.len(), 2);
    assert_eq!(
        execute(&s, &drop_plan(&s), || false).unwrap(),
        Run::Finished(WITHOUT_INDEX)
    );
    assert!(s.shard_bytes().unwrap().is_empty());
    let up = s
        .plan(WITH_INDEX, Direction::Downgrade, NON_ATOMIC)
        .unwrap();
    assert_eq!(
        execute(&s, &up, || false).unwrap(),
        Run::Finished(WITH_INDEX)
    );
    assert_eq!(s.shard_bytes().unwrap(), original);
    assert!(!s.intent_exists());
}

#[test]
fn version_alone_does_not_prove_health() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    std::fs::remove_file(s.shards_dir().join("shard-0")).unwrap();
    assert_eq!(s.read_marker_version(), Ok(WITH_INDEX));
    assert!(matches!(s.observe(), Err(Error::Inconsistent(_))));
}

#[test]
fn killed_mid_removal_is_dirty_and_rolls_back_byte_for_byte() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let original = s.shard_bytes().unwrap();
    let plan = drop_plan(&s);
    s.faults().set("shard.removed", Action::Err, 0);
    let out = execute(&s, &plan, || false).unwrap();
    assert!(matches!(
        out,
        Run::NeedsRecovery {
            last_confirmed: WITH_INDEX,
            attempted: WITHOUT_INDEX,
            ..
        }
    ));
    s.faults().clear();
    // Marker still says "indexed", yet observation is dirty.
    assert_eq!(s.read_marker_version(), Ok(WITH_INDEX));
    assert_eq!(s.state().unwrap(), StoreState::RecoveryRequired);
    assert!(matches!(execute(&s, &plan, || false), Err(Error::Dirty)));
    assert_eq!(s.shard_bytes().unwrap().len(), 1);

    // Recovery itself is interrupted after the rollback marker rename, then
    // retried: verified, not rolled back twice.
    s.faults().set("marker.parent_sync", Action::Err, 0);
    assert!(s.recover_under_guard().is_err());
    assert!(s.intent_exists());
    s.faults().clear();
    assert_eq!(
        s.recover_under_guard(),
        Ok(IndexRecovered::RollbackVerified)
    );
    assert_eq!(s.shard_bytes().unwrap(), original);
    assert_eq!(s.state().unwrap(), StoreState::Ready(WITH_INDEX));
}

#[test]
fn interrupted_recovery_can_be_retried() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let original = s.shard_bytes().unwrap();
    let plan = drop_plan(&s);
    s.faults().set("shard.removed", Action::Err, 0);
    execute(&s, &plan, || false).unwrap();
    s.faults().set("shard.write", Action::Err, 1);
    assert!(s.recover_under_guard().is_err());
    s.faults().clear();
    assert_eq!(s.recover_under_guard(), Ok(IndexRecovered::RolledBack));
    assert_eq!(s.shard_bytes().unwrap(), original);
}

#[test]
fn intent_left_after_a_confirmed_commit_is_finalized_at_the_target() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let plan = drop_plan(&s);
    s.faults().set("intent.clear.unlink", Action::Err, 0);
    assert!(matches!(
        execute(&s, &plan, || false).unwrap(),
        Run::NeedsRecovery { .. }
    ));
    s.faults().clear();
    assert_eq!(s.read_marker_version(), Ok(WITHOUT_INDEX));
    assert_eq!(s.recover_under_guard(), Ok(IndexRecovered::Target));
    assert_eq!(s.state().unwrap(), StoreState::Ready(WITHOUT_INDEX));
    assert!(s.shard_bytes().unwrap().is_empty());
}

#[test]
fn prewrite_failure_is_not_committed_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let plan = drop_plan(&s);
    s.faults().set("intent.stage", Action::Err, 0);
    let w = s.faults().mutations();
    let out = execute(&s, &plan, || false).unwrap();
    assert_eq!(s.faults().mutations(), w);
    assert!(matches!(
        out,
        Run::Stopped {
            last: WITH_INDEX,
            ..
        }
    ));
    assert!(!s.intent_exists());
    assert_eq!(s.shard_bytes().unwrap().len(), 2);
}

#[test]
fn a_peer_edit_makes_the_plan_stale() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITHOUT_INDEX);
    let plan = s
        .plan(WITH_INDEX, Direction::Downgrade, NON_ATOMIC)
        .unwrap();
    s.edit_records("dddd").unwrap();
    let w = s.faults().mutations();
    assert!(matches!(
        execute(&s, &plan, || false),
        Err(Error::StalePlan)
    ));
    assert_eq!(s.faults().mutations(), w);
}

#[test]
fn a_foreign_marker_is_a_chain_mismatch() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    std::fs::write(
        s.marker_path(),
        r#"{"chain":"pins","version":"indexed","revision":1}"#,
    )
    .unwrap();
    assert_eq!(s.observe(), Err(Error::ChainMismatch));
    assert!(matches!(
        s.plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE),
        Err(Error::ChainMismatch)
    ));
}

#[test]
fn inconsistent_prewrite_state_fails_closed() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let mut session = s.begin_migration().unwrap();
    let step = Step {
        from: WITH_INDEX,
        to: WITHOUT_INDEX,
        id: IndexStep::DropShards,
    };
    std::fs::remove_file(s.shards_dir().join("shard-0")).unwrap();
    let w = s.faults().mutations();
    assert!(matches!(
        session.apply(step),
        StepOutcome::RecoveryRequired(Error::Inconsistent(_))
    ));
    assert_eq!(s.faults().mutations(), w);
    assert!(!s.intent_exists());
}

#[test]
fn a_wrong_version_at_prewrite_is_not_committed() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITHOUT_INDEX);
    let mut session = s.begin_migration().unwrap();
    let step = Step {
        from: WITH_INDEX,
        to: WITHOUT_INDEX,
        id: IndexStep::DropShards,
    };
    assert_eq!(
        session.apply(step),
        StepOutcome::NotCommitted(Error::UnexpectedVersion)
    );
}

#[test]
fn missing_and_malformed_chain_names_are_observation_errors() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    let w = s.faults().mutations();
    for (doc, want) in [
        (r#"{"version":"indexed","revision":1}"#, Error::MissingChain),
        (
            r#"{"chain":5,"version":"indexed","revision":1}"#,
            Error::MalformedChain,
        ),
    ] {
        std::fs::write(s.marker_path(), doc).unwrap();
        assert_eq!(s.observe(), Err(want.clone()));
        assert_eq!(s.state(), Err(want.clone()));
        let p = s.plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE);
        assert!(matches!(p, Err(ref e) if *e == want));
    }
    assert_eq!(s.faults().mutations(), w);
    assert!(!s.intent_exists());
}

#[test]
fn create_rejects_unsupported_versions() {
    let dir = tempdir().unwrap();
    let r =
        IndexStore::create(dir.path(), Faults::new(), 1, Version(999), &["a"]);
    assert!(matches!(r, Err(Error::UnknownVersion(Version(999)))));
}

#[test]
fn editor_refuses_a_records_only_store_with_a_leftover_shard() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITHOUT_INDEX);
    std::fs::create_dir_all(s.shards_dir()).unwrap();
    std::fs::write(s.shards_dir().join("shard-0"), b"x").unwrap();
    assert!(matches!(s.edit_records("z"), Err(Error::Inconsistent(_))));
    assert!(!s.intent_exists());
}

#[test]
fn direct_session_apply_never_overwrites_a_pending_intent() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    s.faults().set("marker.parent_sync", Action::Err, 0);
    execute(&s, &drop_plan(&s), || false).unwrap();
    s.faults().clear();
    let before = std::fs::read(s.intent_path()).unwrap();
    let mut session = s.begin_migration().unwrap();
    let step = Step {
        from: WITHOUT_INDEX,
        to: WITH_INDEX,
        id: IndexStep::RebuildShards,
    };
    assert_eq!(
        session.apply(step),
        StepOutcome::RecoveryRequired(Error::Dirty)
    );
    assert_eq!(std::fs::read(s.intent_path()).unwrap(), before);
}

#[test]
fn a_leftover_shard_is_not_a_plain_version_mismatch() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITHOUT_INDEX);
    std::fs::create_dir_all(s.shards_dir()).unwrap();
    std::fs::write(s.shards_dir().join("shard-0"), b"x").unwrap();
    let mut session = s.begin_migration().unwrap();
    let step = Step {
        from: WITH_INDEX,
        to: WITHOUT_INDEX,
        id: IndexStep::DropShards,
    };
    assert!(matches!(
        session.apply(step),
        StepOutcome::RecoveryRequired(Error::Inconsistent(_))
    ));
}

#[test]
fn recovery_rejects_rollback_evidence_that_changes_the_version() {
    let dir = tempdir().unwrap();
    let s = idx(dir.path(), WITH_INDEX);
    s.faults().set("shard.removed", Action::Err, 0);
    execute(&s, &drop_plan(&s), || false).unwrap();
    s.faults().clear();
    let mut intent: serde_json::Value =
        serde_json::from_slice(&std::fs::read(s.intent_path()).unwrap())
            .unwrap();
    intent["rollback"]["version"] = "records-only".into();
    std::fs::write(s.intent_path(), intent.to_string()).unwrap();
    assert_eq!(s.recover_under_guard(), Err(Error::RecoveryBlocked));
    assert!(s.intent_exists());
    assert_eq!(s.shard_bytes().unwrap().len(), 1);
}
