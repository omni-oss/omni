//! Task identity leaf v1 (RFC 0024 F03.4 slice 0).
//!
//! Not yet part of the production digest: [`crate::Collector`] still inserts
//! the legacy joined-label leaf until the coherent cutover.

use omni_hasher::impls::{DefaultHash, hash_parts};

/// Domain separator and recipe version, in that order, ahead of the names.
pub const IDENTITY_LEAF_V1_PREFIX: &[u8] = b"omni.task-id\0\x01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a task name is too long to encode its length")]
pub struct IdentityLeafError;

fn len_le(name: &str) -> Result<[u8; 8], IdentityLeafError> {
    u64::try_from(name.len())
        .map(u64::to_le_bytes)
        .map_err(|_| IdentityLeafError)
}

/// The leaf hash over `prefix || u64_le(len(project)) || project ||
/// u64_le(len(task)) || task`, streamed without building the preimage.
pub fn identity_leaf_v1(
    project: &str,
    task: &str,
) -> Result<DefaultHash, IdentityLeafError> {
    let project_len = len_le(project)?;
    let task_len = len_le(task)?;

    Ok(hash_parts([
        IDENTITY_LEAF_V1_PREFIX,
        &project_len,
        project.as_bytes(),
        &task_len,
        task.as_bytes(),
    ]))
}

#[cfg(test)]
mod tests {
    use omni_hasher::{Hasher, impls::DefaultHasher};

    use super::*;

    fn hex(bytes: &str) -> Vec<u8> {
        bytes
            .split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).expect("hex byte"))
            .collect()
    }

    fn vector(rest: &str) -> Vec<u8> {
        let mut preimage = hex("6f 6d 6e 69 2e 74 61 73 6b 2d 69 64 00 01");
        preimage.extend(hex(rest));
        preimage
    }

    fn assert_vector(project: &str, task: &str, rest: &str) {
        let preimage = vector(rest);

        assert!(preimage.starts_with(IDENTITY_LEAF_V1_PREFIX));
        assert_eq!(
            identity_leaf_v1(project, task),
            Ok(DefaultHasher::hash(&preimage)),
            "{project:?}, {task:?}"
        );
    }

    #[test]
    fn matches_the_plan_vectors() {
        assert_vector(
            "a#b",
            "c",
            "03 00 00 00 00 00 00 00 61 23 62 01 00 00 00 00 00 00 00 63",
        );
        assert_vector(
            "a",
            "b#c",
            "01 00 00 00 00 00 00 00 61 03 00 00 00 00 00 00 00 62 23 63",
        );
        assert_vector(
            "é",
            "🧪",
            "02 00 00 00 00 00 00 00 c3 a9 04 00 00 00 00 00 00 00 f0 9f a7 aa",
        );
        assert_vector(
            "",
            "#",
            "00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 23",
        );
    }

    #[test]
    fn splits_the_legacy_collision_pair() {
        let legacy = |p: &str, t: &str| {
            DefaultHasher::hash(format!("{p}#{t}").as_bytes())
        };

        assert_eq!(legacy("a#b", "c"), legacy("a", "b#c"));
        assert_ne!(
            identity_leaf_v1("a#b", "c").unwrap(),
            identity_leaf_v1("a", "b#c").unwrap()
        );
    }
}
