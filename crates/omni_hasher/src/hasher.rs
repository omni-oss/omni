pub trait Hasher: Clone {
    type Hash: Copy + PartialEq + Into<Vec<u8>> + TryFrom<Vec<u8>> + Send;
    fn hash(data: &[u8]) -> Self::Hash;
}

pub mod impls {
    use super::*;

    #[derive(Clone, Copy)]
    #[repr(transparent)]
    pub struct Blake3Hasher;

    impl Hasher for Blake3Hasher {
        type Hash = [u8; 32];

        #[inline(always)]
        fn hash(data: &[u8]) -> Self::Hash {
            *blake3::hash(data).as_bytes()
        }
    }
}
