use std::{fmt::Debug, hash::Hash};

use serde::{Deserialize, Serialize};

pub trait Hasher: Clone {
    type Hash: Copy
        + PartialEq
        + Into<Vec<u8>>
        + TryFrom<Vec<u8>>
        + AsRef<[u8]>
        + Send
        + Sync
        + PartialEq
        + Eq
        + Serialize
        + for<'a> Deserialize<'a>
        + Debug
        + Hash;
    fn hash(data: &[u8]) -> Self::Hash;
}

pub mod impls {
    use super::*;

    #[derive(Clone, Copy)]
    #[repr(transparent)]
    pub struct Blake3Hasher;

    pub type Blake3Hash = [u8; 32];

    impl Hasher for Blake3Hasher {
        type Hash = Blake3Hash;

        #[inline(always)]
        fn hash(data: &[u8]) -> Self::Hash {
            *blake3::hash(data).as_bytes()
        }
    }

    /// Streams `parts` through one plain, unkeyed BLAKE3 hash. The result is
    /// exactly [`Blake3Hasher::hash`] over the concatenation of the parts,
    /// without allocating that concatenation.
    pub fn hash_parts<'a>(
        parts: impl IntoIterator<Item = &'a [u8]>,
    ) -> Blake3Hash {
        let mut hasher = blake3::Hasher::new();
        for part in parts {
            hasher.update(part);
        }
        *hasher.finalize().as_bytes()
    }

    pub type DefaultHasher = Blake3Hasher;
    pub type DefaultHash = <DefaultHasher as Hasher>::Hash;
}

#[cfg(test)]
mod tests {
    use super::{
        Hasher,
        impls::{Blake3Hasher, hash_parts},
    };

    #[test]
    fn hash_parts_equals_hashing_the_concatenation() {
        let parts: [&[u8]; 4] = [b"omni", b"", b"\0\x01", b"tail"];
        let whole = parts.concat();

        assert_eq!(hash_parts(parts), Blake3Hasher::hash(&whole));
        assert_eq!(hash_parts([]), Blake3Hasher::hash(b""));
    }

    /// The v1 task identity leaf is hashed with `hash_parts`; keep it tied to
    /// the default hasher so the two cannot silently diverge.
    #[test]
    fn hash_parts_matches_the_default_hasher() {
        use super::impls::{DefaultHash, DefaultHasher};

        let parts: [&[u8]; 3] = [b"omni", b"\0", b"v1"];
        let via_parts: DefaultHash = hash_parts(parts);
        assert_eq!(via_parts, DefaultHasher::hash(&parts.concat()));
    }
}
