use std::marker::PhantomData;

use crate::hash::Hasher;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Compat<T: Hasher>(PhantomData<T>);

impl<T: Hasher> rs_merkle::Hasher for Compat<T> {
    type Hash = T::Hash;

    fn hash(data: &[u8]) -> Self::Hash {
        T::hash(data)
    }
}
