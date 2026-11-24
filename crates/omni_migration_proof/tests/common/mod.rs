#![allow(dead_code)]

use omni_migration_proof::core::Policy;
use omni_migration_proof::fs::Faults;
use omni_migration_proof::lockfile::{
    Body, LockfileStore, PINS_V1, PinsCatalog, V1,
};

pub const NON_ATOMIC: Policy = Policy {
    allow_non_atomic: true,
    allow_derivable_loss: false,
    allow_irreversible_loss: false,
};

pub const NON_ATOMIC_DERIVABLE: Policy = Policy {
    allow_non_atomic: true,
    allow_derivable_loss: true,
    allow_irreversible_loss: false,
};

pub fn v1_store(root: &std::path::Path, catalog: PinsCatalog) -> LockfileStore {
    LockfileStore::create(
        root,
        Faults::new(),
        catalog,
        1000,
        PINS_V1,
        Body::V1(V1 {
            pins: vec!["a".into()],
        }),
    )
    .unwrap()
}

/// A peer rewrites the document by hand with a new generation.
pub fn rewrite_doc(
    store: &LockfileStore,
    f: impl FnOnce(&mut serde_json::Value),
) {
    let mut v: serde_json::Value =
        serde_json::from_slice(&store.document_bytes()).unwrap();
    f(&mut v);
    let g = v["generation"].as_u64().unwrap() + 1;
    v["generation"] = g.into();
    std::fs::write(store.doc_path(), serde_json::to_vec(&v).unwrap()).unwrap();
}
