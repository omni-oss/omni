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

    pub type DefaultHasher = Blake3Hasher;
    pub type DefaultHash = <DefaultHasher as Hasher>::Hash;
}
