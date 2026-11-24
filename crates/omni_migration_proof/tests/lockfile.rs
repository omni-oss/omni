mod common;

use std::cell::Cell;

use common::*;
use omni_migration_proof::core::*;
use omni_migration_proof::fs::{Action, DurableFs, Faults};
use omni_migration_proof::lockfile::*;
use tempfile::tempdir;

fn run(store: &LockfileStore, to: Version, d: Direction) -> Run {
    let plan = store.plan(to, d, Policy::default()).unwrap();
    execute(store, &plan, || false).unwrap()
}

#[test]
fn round_trip_and_refusal() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    assert_eq!(
        run(&store, PINS_V2, Direction::Upgrade),
        Run::Finished(PINS_V2)
    );
    let (v, body) = store.read_body().unwrap();
    assert_eq!(v, PINS_V2);
    assert_eq!(
        body,
        Body::V2(V2 {
            pins: vec!["a".into()],
            notes: vec![]
        })
    );
    assert_eq!(
        run(&store, PINS_V1, Direction::Downgrade),
        Run::Finished(PINS_V1)
    );
    assert_eq!(store.read_body().unwrap().1.pins(), ["a".to_string()]);
    let doc: serde_json::Value =
        serde_json::from_slice(&store.document_bytes()).unwrap();
    assert_eq!(doc["generation"], 3);

    run(&store, PINS_V2, Direction::Upgrade);
    rewrite_doc(&store, |v| v["body"]["notes"] = serde_json::json!(["keep"]));
    let before = store.document_bytes();
    let w = store.faults().mutations();
    assert_eq!(
        run(&store, PINS_V1, Direction::Downgrade),
        Run::Stopped {
            last: PINS_V2,
            error: Error::MeaningfulNotes
        }
    );
    assert_eq!(store.document_bytes(), before);
    assert!(!store.intent_exists());
    assert_eq!(store.faults().mutations(), w);

    let state = store.state().unwrap();
    let refused =
        gate_access(state, PINS_V1, |_| Err::<(), _>(Error::WrongDirection));
    assert!(refused.is_err());
    assert_eq!(store.document_bytes(), before);
}

#[test]
fn interruptions_classify_and_recover() {
    // (failpoint, expects NeedsRecovery, document is new on disk)
    let cases = [
        ("doc.stage", false, false),
        ("intent.rename", false, false),
        ("intent.parent_sync", true, false),
        ("doc.rename", true, false),
        ("doc.parent_sync", true, true),
        ("intent.clear.unlink", true, true),
    ];
    for (point, needs_recovery, doc_new) in cases {
        let dir = tempdir().unwrap();
        let store = v1_store(dir.path(), PinsCatalog::two());
        let before = store.document_bytes();
        let plan = store
            .plan(PINS_V2, Direction::Upgrade, Policy::default())
            .unwrap();
        store.faults().set(point, Action::Err, 0);
        let out = execute(&store, &plan, || false).unwrap();
        store.faults().clear();
        if needs_recovery {
            assert!(
                matches!(out, Run::NeedsRecovery { last_confirmed, attempted, .. }
                    if last_confirmed == PINS_V1 && attempted == PINS_V2),
                "{point}: {out:?}"
            );
            assert_eq!(store.state().unwrap(), StoreState::RecoveryRequired);
            let which = store.recover_under_guard().unwrap();
            assert_eq!(
                which,
                if doc_new {
                    Recovered::New
                } else {
                    Recovered::Old
                },
                "{point}"
            );
            assert!(!store.intent_exists());
            assert_eq!(
                store.state().unwrap(),
                StoreState::Ready(if doc_new { PINS_V2 } else { PINS_V1 })
            );
        } else {
            assert!(
                matches!(out, Run::Stopped { last: PINS_V1, .. }),
                "{point}: {out:?}"
            );
            assert_eq!(store.document_bytes(), before);
            assert!(!store.intent_exists());
        }
    }
}

#[test]
fn unsynced_intent_removal_after_durable_document_still_commits() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    store
        .faults()
        .set("intent.clear.parent_sync", Action::Err, 0);
    assert_eq!(
        execute(&store, &plan, || false).unwrap(),
        Run::Finished(PINS_V2)
    );
}

#[test]
fn mismatched_recovery_evidence_stays_blocked() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    store.faults().set("doc.rename", Action::Err, 0);
    execute(&store, &plan, || false).unwrap();
    store.faults().clear();
    rewrite_doc(&store, |v| v["body"]["pins"] = serde_json::json!(["x"]));
    assert_eq!(store.recover_under_guard(), Err(Error::RecoveryBlocked));
    assert!(store.intent_exists());
}

#[test]
fn writers_refuse_a_pending_intent() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    store.faults().set("doc.rename", Action::Err, 0);
    execute(&store, &plan, || false).unwrap();
    store.faults().clear();
    assert_eq!(store.edit_pins("b"), Err(Error::Dirty));
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::Dirty)
    ));
}

fn three_with(
    version: Version,
    body: Body,
) -> (tempfile::TempDir, LockfileStore) {
    let dir = tempdir().unwrap();
    let store = LockfileStore::create(
        dir.path(),
        Faults::new(),
        PinsCatalog::three(),
        7,
        version,
        body,
    )
    .unwrap();
    (dir, store)
}

fn v3(notes: &[&str]) -> Body {
    Body::V3(V3 {
        pins: vec!["a".into()],
        notes: notes.iter().map(|s| s.to_string()).collect(),
        tags: vec![],
    })
}

#[test]
fn prefix_commit_is_not_a_chain_transaction() {
    let (_d, store) = three_with(PINS_V3, v3(&["n"]));
    let plan = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(
        execute(&store, &plan, || false).unwrap(),
        Run::Stopped {
            last: PINS_V2,
            error: Error::MeaningfulNotes
        }
    );
    assert_eq!(store.state().unwrap(), StoreState::Ready(PINS_V2));

    let (_d, store) = three_with(PINS_V3, v3(&[]));
    let plan = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    store.faults().set("doc.parent_sync", Action::Err, 1);
    let out = execute(&store, &plan, || false).unwrap();
    assert!(matches!(
        out,
        Run::NeedsRecovery {
            last_confirmed: PINS_V2,
            attempted: PINS_V1,
            ..
        }
    ));
}

#[test]
fn cancellation_keeps_the_committed_prefix() {
    let (_d, store) = three_with(PINS_V3, v3(&[]));
    let plan = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    let polls = Cell::new(0);
    let out = execute(&store, &plan, || {
        polls.set(polls.get() + 1);
        polls.get() > 1
    })
    .unwrap();
    assert_eq!(
        out,
        Run::Stopped {
            last: PINS_V2,
            error: Error::Cancelled
        }
    );
    assert_eq!(store.state().unwrap(), StoreState::Ready(PINS_V2));
}

#[test]
fn a_live_session_excludes_other_writers() {
    let (_d, store) = three_with(PINS_V3, v3(&[]));
    let session = store.begin_migration().unwrap();
    let other = DurableFs::new(Faults::new());
    assert_eq!(
        other.try_lock_exclusive(&store.guard_path()).err(),
        Some(Error::GuardBusy)
    );
    drop(session);
    assert!(other.try_lock_exclusive(&store.guard_path()).is_ok());
}

#[test]
fn guarded_stale_checks_write_nothing() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();

    store.edit_pins("b").unwrap();
    let w = store.faults().mutations();
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StalePlan)
    ));
    assert_eq!(store.faults().mutations(), w);

    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    // Same logical data, new generation.
    rewrite_doc(&store, |_| {});
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StalePlan)
    ));

    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    // Replaced store: new incarnation.
    rewrite_doc(&store, |v| v["incarnation"] = 4242.into());
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StalePlan)
    ));

    // A peer reaches the target first; no replanning even though it is current.
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    run(&store, PINS_V2, Direction::Upgrade);
    let w = store.faults().mutations();
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StalePlan)
    ));
    assert_eq!(store.faults().mutations(), w);

    // Capability changed between planning and guarded preflight.
    let plan = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    let mut c = PinsCatalog::two();
    c.caps.push((
        PinsStep::BackToPlainPins,
        StepCapabilities {
            atomic: true,
            lossless: false,
            derivable: false,
        },
    ));
    store.set_catalog(c);
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StaleCatalog)
    ));
    assert_eq!(store.faults().mutations(), w);
}
