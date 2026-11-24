mod common;

use common::*;
use omni_migration_proof::core::*;
use omni_migration_proof::fs::Faults;
use omni_migration_proof::index::*;
use omni_migration_proof::lockfile::*;
use tempfile::tempdir;

fn idx(root: &std::path::Path, v: Version) -> IndexStore {
    IndexStore::create(root, Faults::new(), 5, v, &["bb", "a", "ccc"]).unwrap()
}

#[test]
fn zero_work_path_never_touches_adapters() {
    let r = gate_access(
        StoreState::Ready(PINS_V2),
        PINS_V2,
        |_| -> Result<(), Error> { panic!("cold path entered") },
    );
    assert_eq!(r, Ok(Check::UpToDate));
    let r = gate_access(
        StoreState::RecoveryRequired,
        PINS_V2,
        |_| -> Result<(), Error> { panic!("cold path entered") },
    );
    assert_eq!(r, Err(Error::Dirty));
}

#[test]
fn explicit_requests_to_the_observed_version_are_wrong_direction() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    for d in [Direction::Upgrade, Direction::Downgrade] {
        assert!(matches!(
            store.plan(PINS_V1, d, Policy::default()),
            Err(Error::WrongDirection)
        ));
    }
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    store
        .faults()
        .set("doc.rename", omni_migration_proof::fs::Action::Err, 0);
    execute(&store, &plan, || false).unwrap();
    for d in [Direction::Upgrade, Direction::Downgrade] {
        assert!(matches!(
            store.plan(PINS_V1, d, Policy::default()),
            Err(Error::Dirty)
        ));
    }
}

#[test]
fn policy_is_directional() {
    let dir = tempdir().unwrap();
    let down = idx(dir.path(), WITH_INDEX);
    let p = |pol| {
        down.plan(WITHOUT_INDEX, Direction::Upgrade, pol)
            .map(|p| p.steps.len())
    };
    assert!(matches!(p(Policy::default()), Err(Error::NonAtomic)));
    assert!(matches!(p(NON_ATOMIC), Err(Error::DerivableLoss)));
    assert_eq!(p(NON_ATOMIC_DERIVABLE), Ok(1));
    assert!(matches!(
        p(Policy {
            allow_derivable_loss: true,
            ..Policy::default()
        }),
        Err(Error::NonAtomic)
    ));

    let dir = tempdir().unwrap();
    let up = idx(dir.path(), WITHOUT_INDEX);
    let p = |pol| {
        up.plan(WITH_INDEX, Direction::Downgrade, pol)
            .map(|p| p.steps.len())
    };
    assert_eq!(p(NON_ATOMIC), Ok(1));
    assert!(matches!(
        p(Policy {
            allow_derivable_loss: true,
            ..Policy::default()
        }),
        Err(Error::NonAtomic)
    ));

    let irreversible = StepCapabilities {
        atomic: true,
        lossless: false,
        derivable: false,
    };
    assert_eq!(
        Policy::default().check(irreversible),
        Err(Error::IrreversibleLoss)
    );
    assert_eq!(
        Policy {
            allow_derivable_loss: true,
            ..Policy::default()
        }
        .check(irreversible),
        Err(Error::IrreversibleLoss)
    );
    assert_eq!(
        Policy {
            allow_irreversible_loss: true,
            ..Policy::default()
        }
        .check(irreversible),
        Ok(())
    );
}

#[test]
fn chain_validation_rejects_bad_shapes() {
    type T = Transition<PinsStep>;
    let t = |p, v, u, d| T {
        previous: Version(p),
        version: Version(v),
        upgrade: u,
        downgrade: d,
    };
    use PinsStep::*;
    assert!(Chain::new(Version(1), &[t(1, 2, AddNotes, None)]).is_ok());
    // Duplicate version (including the base), duplicate step id (across
    // directions) and a disconnected predecessor.
    let bad: [&[T]; 4] = [
        &[t(1, 1, AddNotes, None)],
        &[t(1, 2, AddNotes, Some(AddNotes))],
        &[t(1, 2, AddNotes, None), t(2, 3, AddNotes, None)],
        &[t(1, 2, AddNotes, None), t(9, 3, AddTags, None)],
    ];
    for edges in bad {
        assert!(matches!(
            Chain::new(Version(1), edges),
            Err(Error::InvalidChain)
        ));
    }
}

#[test]
fn full_plan_before_writes() {
    let dir = tempdir().unwrap();
    let mut c = PinsCatalog::three();
    c.edges[0].downgrade = None; // second reverse leg (v2 -> v1)
    let store = LockfileStore::create(
        dir.path(),
        Faults::new(),
        c,
        1,
        PINS_V3,
        Body::V3(V3 {
            pins: vec![],
            notes: vec![],
            tags: vec![],
        }),
    )
    .unwrap();
    let w = store.faults().mutations();
    assert!(matches!(
        store.plan(PINS_V1, Direction::Downgrade, Policy::default()),
        Err(Error::NoDowngradePath)
    ));
    // Declared but not implemented.
    let mut c = PinsCatalog::three();
    c.missing.push(PinsStep::BackToPlainPins);
    store.set_catalog(c);
    assert!(matches!(
        store.plan(PINS_V1, Direction::Downgrade, Policy::default()),
        Err(Error::MissingStep)
    ));
    assert_eq!(store.faults().mutations(), w);

    store.set_catalog(PinsCatalog::three());
    let a = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    let b = store
        .plan(PINS_V1, Direction::Downgrade, Policy::default())
        .unwrap();
    assert_eq!(a.steps, b.steps);
    let order: Vec<_> = a.steps.iter().map(|(s, _)| s.id).collect();
    assert_eq!(order, [PinsStep::DropTags, PinsStep::BackToPlainPins]);
    // The 2-version catalog does not know v3.
    store.set_catalog(PinsCatalog::two());
    assert!(matches!(
        store.plan(PINS_V1, Direction::Downgrade, Policy::default()),
        Err(Error::UnknownVersion(PINS_V3))
    ));
}

#[test]
fn observation_errors_are_distinct() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let put = |v: serde_json::Value| {
        std::fs::write(store.doc_path(), v.to_string()).unwrap()
    };
    let doc = |tag: serde_json::Value| {
        serde_json::json!({
            "chain": "pins", "version": tag, "generation": 1, "incarnation": 1,
            "body": {"pins": []}
        })
    };
    let mut missing_tag = doc("x".into());
    missing_tag.as_object_mut().unwrap().remove("version");
    put(missing_tag);
    assert_eq!(store.observe(), Err(Error::MissingVersion));
    put(doc(7.into()));
    assert_eq!(store.observe(), Err(Error::MalformedVersion));
    put(doc("bogus".into()));
    assert_eq!(store.observe(), Err(Error::MalformedVersion));
    put(doc("pins-v99".into()));
    assert_eq!(store.observe(), Err(Error::UnknownTag));
    std::fs::write(store.doc_path(), "{not json").unwrap();
    assert_eq!(store.observe(), Err(Error::MalformedVersion));
    std::fs::remove_file(store.doc_path()).unwrap();
    assert_eq!(store.observe(), Err(Error::MissingVersion));
}

#[test]
fn chain_identity_is_verified_at_observation() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let w = store.faults().mutations();
    for (chain, want) in [
        (Some(serde_json::json!("index")), Error::ChainMismatch),
        (Some(serde_json::json!(5)), Error::MalformedChain),
        (None, Error::MissingChain),
    ] {
        rewrite_doc(&store, |v| match &chain {
            Some(c) => v["chain"] = c.clone(),
            None => {
                v.as_object_mut().unwrap().remove("chain");
            }
        });
        assert_eq!(store.observe(), Err(want.clone()));
        assert!(matches!(
            store.plan(PINS_V2, Direction::Upgrade, Policy::default()),
            Err(ref e) if *e == want
        ));
        assert!(matches!(
            execute(&store, &dummy_plan(), || false),
            Err(ref e) if *e == want
        ));
        rewrite_doc(&store, |v| v["chain"] = "pins".into());
    }
    // rewrite_doc is a test peer, not a migration writer: only the guard-free
    // observation and planning paths are under test, and neither wrote.
    assert_eq!(store.faults().mutations(), w);
    assert!(!store.intent_exists());
}

fn dummy_plan() -> Plan<PinsStep> {
    let stamp = Stamp {
        store: StoreId(1000),
        chain: ChainName::PINS,
        revision: Revision(1),
        version: PINS_V1,
    };
    let c = PinsCatalog::two();
    Plan {
        observed: stamp,
        catalog: c.id(),
        binding: c.binding(),
        direction: Direction::Upgrade,
        target: PINS_V2,
        policy: Policy::default(),
        steps: vec![],
    }
}

#[test]
fn edited_chain_is_stale_catalog_not_chain_mismatch() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    let stamp = match store.observe().unwrap() {
        GuardedObservation::Ready(s) => s,
        _ => unreachable!(),
    };
    let mut edited = PinsCatalog::two();
    edited.edges[0].upgrade = PinsStep::AddTags;
    let original = PinsCatalog::two();
    // At planning: chain A against catalog B.
    let r = build_plan(
        &original.chain().unwrap(),
        &edited,
        stamp,
        PINS_V2,
        Direction::Upgrade,
        Policy::default(),
    );
    assert!(matches!(r, Err(Error::StaleCatalog)));

    // At guarded preflight.
    let plan = store
        .plan(PINS_V2, Direction::Upgrade, Policy::default())
        .unwrap();
    store.set_catalog(edited);
    let w = store.faults().mutations();
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StaleCatalog)
    ));
    assert_eq!(store.faults().mutations(), w);

    // Appending a transition keeps the old store readable.
    store.set_catalog(PinsCatalog::three());
    assert!(
        store
            .plan(PINS_V2, Direction::Upgrade, Policy::default())
            .is_ok()
    );
}

#[test]
fn intent_chain_name_decides_before_the_document() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    rewrite_doc(&store, |v| v["chain"] = 5.into());
    std::fs::write(store.intent_path(), r#"{"chain":"pins"}"#).unwrap();
    assert_eq!(store.observe(), Ok(GuardedObservation::RecoveryRequired));
    std::fs::write(store.intent_path(), r#"{"chain":"index"}"#).unwrap();
    assert_eq!(store.observe(), Err(Error::ChainMismatch));
    std::fs::write(store.intent_path(), r#"{"nope":1}"#).unwrap();
    assert_eq!(store.observe(), Err(Error::MalformedIntent));

    let dir = tempdir().unwrap();
    let index = idx(dir.path(), WITH_INDEX);
    std::fs::write(index.marker_path(), r#"{"chain":5}"#).unwrap();
    std::fs::write(index.intent_path(), r#"{"chain":"index"}"#).unwrap();
    assert_eq!(index.observe(), Ok(GuardedObservation::RecoveryRequired));
    std::fs::write(index.intent_path(), r#"{"chain":"pins"}"#).unwrap();
    assert_eq!(index.observe(), Err(Error::ChainMismatch));
}

#[test]
fn backend_gate_for_both_fixtures() {
    // durable, no atomic publish: the lockfile (atomic steps) is refused with
    // or without the opt-in; the index (non-atomic steps) runs.
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    store
        .faults()
        .set_support(BackendSupport::DURABLE_NON_ATOMIC);
    let w = store.faults().mutations();
    for pol in [Policy::default(), NON_ATOMIC] {
        assert!(matches!(
            store.plan(PINS_V2, Direction::Upgrade, pol),
            Err(Error::UnsupportedBackend)
        ));
    }
    assert_eq!(store.faults().mutations(), w);
    assert!(!store.intent_exists());

    let dir = tempdir().unwrap();
    let index = idx(dir.path(), WITH_INDEX);
    index
        .faults()
        .set_support(BackendSupport::DURABLE_NON_ATOMIC);
    let plan = index
        .plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE)
        .unwrap();
    assert_eq!(
        execute(&index, &plan, || false).unwrap(),
        Run::Finished(WITHOUT_INDEX)
    );

    // not durable: every step of both fixtures is refused whatever the policy.
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    store.faults().set_support(BackendSupport::NONE);
    let w = store.faults().mutations();
    for pol in [Policy::default(), NON_ATOMIC, NON_ATOMIC_DERIVABLE] {
        assert!(matches!(
            store.plan(PINS_V2, Direction::Upgrade, pol),
            Err(Error::UnsupportedBackend)
        ));
    }
    assert_eq!(store.faults().mutations(), w);
    let dir = tempdir().unwrap();
    let index = idx(dir.path(), WITH_INDEX);
    index.faults().set_support(BackendSupport::NONE);
    assert!(matches!(
        index.plan(WITHOUT_INDEX, Direction::Upgrade, NON_ATOMIC_DERIVABLE),
        Err(Error::UnsupportedBackend)
    ));
}

#[test]
fn support_lost_after_planning_is_caught_with_the_right_outcome() {
    let non_atomic = StepCapabilities {
        atomic: false,
        lossless: true,
        derivable: false,
    };
    let make = || {
        let dir = tempdir().unwrap();
        let mut c = PinsCatalog::three();
        c.caps.push((PinsStep::AddNotes, non_atomic));
        let store = LockfileStore::create(
            dir.path(),
            Faults::new(),
            c,
            1,
            PINS_V1,
            Body::V1(V1 { pins: vec![] }),
        )
        .unwrap();
        (dir, store)
    };
    let (full, weak) =
        (BackendSupport::FULL, BackendSupport::DURABLE_NON_ATOMIC);

    // Probes: plan, begin_migration (both cached), preflight, then one per
    // applied step. A flip at preflight is an error with zero writes even
    // though an earlier step is non-atomic.
    let (_d, store) = make();
    store.faults().script_probes(&[full]);
    let plan = store.plan(PINS_V3, Direction::Upgrade, NON_ATOMIC).unwrap();
    store.faults().script_probes(&[full, weak]);
    let w = store.faults().mutations();
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::UnsupportedBackend)
    ));
    assert_eq!(store.faults().mutations(), w);
    assert_eq!(store.state().unwrap(), StoreState::Ready(PINS_V1));

    // Lost inside a later step, before it mutates.
    let (_d, store) = make();
    let plan = store.plan(PINS_V3, Direction::Upgrade, NON_ATOMIC).unwrap();
    store.faults().script_probes(&[full, full, full, weak]);
    assert_eq!(
        execute(&store, &plan, || false).unwrap(),
        Run::Stopped {
            last: PINS_V2,
            error: Error::UnsupportedBackend
        }
    );
    assert_eq!(store.state().unwrap(), StoreState::Ready(PINS_V2));
}

fn three_store(
    dir: &std::path::Path,
    c: PinsCatalog,
    v: Version,
    body: Body,
) -> LockfileStore {
    LockfileStore::create(dir, Faults::new(), c, 1, v, body).unwrap()
}

#[test]
fn irreversible_second_leg_blocks_the_whole_chain_before_any_write() {
    let dir = tempdir().unwrap();
    let mut c = PinsCatalog::three();
    c.caps.push((
        PinsStep::AddTags,
        StepCapabilities {
            atomic: true,
            lossless: false,
            derivable: false,
        },
    ));
    let store =
        three_store(dir.path(), c, PINS_V1, Body::V1(V1 { pins: vec![] }));
    let w = store.faults().mutations();
    let derivable_only = Policy {
        allow_derivable_loss: true,
        ..Policy::default()
    };
    for pol in [Policy::default(), derivable_only] {
        assert!(matches!(
            store.plan(PINS_V3, Direction::Upgrade, pol),
            Err(Error::IrreversibleLoss)
        ));
    }
    assert_eq!(store.faults().mutations(), w);
    assert_eq!(store.state().unwrap(), StoreState::Ready(PINS_V1));

    let allowed = Policy {
        allow_irreversible_loss: true,
        ..Policy::default()
    };
    let plan = store.plan(PINS_V3, Direction::Upgrade, allowed).unwrap();
    assert_eq!(
        execute(&store, &plan, || false).unwrap(),
        Run::Finished(PINS_V3)
    );
}

#[test]
fn a_reordered_version_edit_is_stale_at_preflight_and_unknown_target_is_refused()
 {
    let dir = tempdir().unwrap();
    let store = three_store(
        dir.path(),
        PinsCatalog::three(),
        PINS_V1,
        Body::V1(V1 { pins: vec![] }),
    );
    let plan = store
        .plan(PINS_V3, Direction::Upgrade, Policy::default())
        .unwrap();
    let mut edited = PinsCatalog::three();
    edited.edges[1].version = Version(14);
    store.set_catalog(edited);
    let w = store.faults().mutations();
    assert!(matches!(
        execute(&store, &plan, || false),
        Err(Error::StaleCatalog)
    ));
    assert_eq!(store.faults().mutations(), w);

    store.set_catalog(PinsCatalog::three());
    assert!(matches!(
        store.plan(Version(999), Direction::Upgrade, Policy::default()),
        Err(Error::UnknownVersion(Version(999)))
    ));
}

#[test]
fn ordinary_access_refuses_a_foreign_store_without_writing() {
    let dir = tempdir().unwrap();
    let store = v1_store(dir.path(), PinsCatalog::two());
    rewrite_doc(&store, |v| v["chain"] = "index".into());
    let w = store.faults().mutations();
    assert_eq!(store.state(), Err(Error::ChainMismatch));
    assert_eq!(store.faults().mutations(), w);
    assert!(!store.intent_exists());
}
