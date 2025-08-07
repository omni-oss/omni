use std::{
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
};

use derive_new::new;
use rs_merkle::Hasher as _;
use serde::{Deserialize, Serialize};

use crate::hash::{Compat, Hasher};

#[derive(Clone, Copy, new)]
#[repr(transparent)]
pub struct Hash<THasher: Hasher> {
    bytes: THasher::Hash,
    #[new(default)]
    _hasher: PhantomData<THasher>,
}

impl<THasher: Hasher> Serialize for Hash<THasher> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.bytes.serialize(serializer)
    }
}

impl<'de, THasher: Hasher> Deserialize<'de> for Hash<THasher> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self {
            bytes: THasher::Hash::deserialize(deserializer)?,
            _hasher: PhantomData,
        })
    }
}

impl<THasher: Hasher + std::hash::Hash> std::hash::Hash for Hash<THasher> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl<THasher: Hasher> Eq for Hash<THasher> {}

impl<THasher: Hasher> PartialEq for Hash<THasher> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<THasher: Hasher> Debug for Hash<THasher> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hash").field("bytes", &self.bytes).finish()
    }
}

impl<THasher: Hasher> Hash<THasher> {
    #[inline(always)]
    pub fn as_inner(&self) -> &THasher::Hash {
        &self.bytes
    }

    #[inline(always)]
    pub fn combine_in_place<T: AsRef<[u8]>>(&mut self, to_hash: T) {
        let hash = THasher::hash(to_hash.as_ref());
        self.bytes =
            Compat::<THasher>::concat_and_hash(&self.bytes, Some(&hash));
    }

    #[inline(always)]
    pub fn combine<T: AsRef<[u8]>>(&self, to_hash: T) -> Self {
        let mut clone = self.clone();

        clone.combine_in_place(to_hash);

        clone
    }
}
