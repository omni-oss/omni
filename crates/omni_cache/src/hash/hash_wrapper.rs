use std::marker::PhantomData;

use derive_new::new;
use rs_merkle::Hasher as _;
use serde::{Deserialize, Serialize};

use crate::hash::{Compat, Hasher};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    new,
    Deserialize,
    Serialize,
)]
#[repr(transparent)]
pub struct Hash<THasher: Hasher> {
    bytes: THasher::Hash,
    #[new(default)]
    hasher: PhantomData<THasher>,
}

impl<THasher: Hasher> Hash<THasher> {
    #[inline(always)]
    pub fn as_inner(&self) -> &THasher::Hash {
        &self.bytes
    }

    #[inline(always)]
    pub fn combine<T: AsRef<[u8]>>(&mut self, to_hash: T) {
        let data = to_hash.as_ref();
        let hash = THasher::hash(data);

        self.bytes =
            Compat::<THasher>::concat_and_hash(&self.bytes, Some(&hash));
    }
}
