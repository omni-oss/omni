//! The two trailing key leaves of RFC 0024 F06.3, inserted by
//! [`crate::Collector`] after the identity leaf, in this order:
//!
//! 1. the cache key version: `BLAKE3(b"omni.cache-key\0" || u32_le(version))`;
//! 2. the effective task:
//!    `BLAKE3(b"omni.effective-task\0" || 0x01 || effective_digest[32])`.
//!
//! The `0x01` versions the effective leaf's own framing (domain, position,
//! width). An encoding change inside the digest bumps
//! [`crate::effective_task::VERSION`] instead. Either is a cold start.

use omni_hasher::impls::{DefaultHash, hash_parts};

pub(crate) const CACHE_KEY_VERSION_PREFIX: &[u8] = b"omni.cache-key\0";
pub(crate) const EFFECTIVE_TASK_LEAF_V1_PREFIX: &[u8] =
    b"omni.effective-task\0\x01";

pub(crate) fn cache_key_version_leaf(version: u32) -> DefaultHash {
    hash_parts([CACHE_KEY_VERSION_PREFIX, &version.to_le_bytes()])
}

pub(crate) fn effective_task_leaf_v1(
    effective_digest: &DefaultHash,
) -> DefaultHash {
    hash_parts([EFFECTIVE_TASK_LEAF_V1_PREFIX, effective_digest.as_slice()])
}

#[cfg(test)]
mod tests {
    use omni_hasher::{Hasher, impls::DefaultHasher};

    use super::*;

    #[test]
    fn the_version_leaf_is_the_hash_of_the_domain_and_the_little_endian_version()
     {
        assert_eq!(
            cache_key_version_leaf(1),
            DefaultHasher::hash(b"omni.cache-key\0\x01\x00\x00\x00")
        );
        assert_ne!(cache_key_version_leaf(1), cache_key_version_leaf(2));
    }

    #[test]
    fn the_effective_leaf_is_the_hash_of_the_domain_the_version_byte_and_the_digest()
     {
        let digest = [7u8; 32];
        let mut preimage = b"omni.effective-task\0\x01".to_vec();
        preimage.extend_from_slice(&digest);
        assert_eq!(
            effective_task_leaf_v1(&digest),
            DefaultHasher::hash(&preimage)
        );
    }
}
