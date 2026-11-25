use std::collections::HashSet;

use crate::{ChainBinding, ChainName, Error, Version};

/// A step identifier with a stable numeric encoding.
///
/// The encoding feeds the chain digest, so it must never change for a
/// released step and must be unique within a chain.
pub trait StepId: Copy + Eq {
    fn stable_id(&self) -> u32;
}

#[derive(Clone, Copy, Debug)]
pub struct Transition<S> {
    pub previous: Version,
    pub version: Version,
    pub upgrade: S,
    pub downgrade: Option<S>,
}

/// A validated linear chain: connected edges, unique versions and unique step
/// IDs.
pub struct Chain<'a, S> {
    base: Version,
    edges: &'a [Transition<S>],
}

impl<'a, S: StepId> Chain<'a, S> {
    pub fn new(
        base: Version,
        edges: &'a [Transition<S>],
    ) -> Result<Self, Error> {
        let mut versions = HashSet::from([base]);
        let mut ids = HashSet::new();
        let mut previous = base;
        for edge in edges {
            if edge.previous != previous
                || !versions.insert(edge.version)
                || !ids.insert(edge.upgrade.stable_id())
                || edge.downgrade.is_some_and(|id| !ids.insert(id.stable_id()))
            {
                return Err(Error::InvalidChain);
            }
            previous = edge.version;
        }
        Ok(Self { base, edges })
    }

    pub fn base(&self) -> Version {
        self.base
    }

    pub fn edges(&self) -> &'a [Transition<S>] {
        self.edges
    }

    /// Digest of the base, the ordered edges and their step IDs, as blake3
    /// over a fixed little-endian encoding that does not depend on the Rust
    /// version or platform.
    pub fn binding(&self, name: ChainName) -> ChainBinding {
        let mut h = blake3::Hasher::new();
        h.update(b"omni_migration.chain.v1\0");
        h.update(&self.base.0.to_le_bytes());
        h.update(&(self.edges.len() as u64).to_le_bytes());
        for e in self.edges {
            h.update(&e.previous.0.to_le_bytes());
            h.update(&e.version.0.to_le_bytes());
            h.update(&e.upgrade.stable_id().to_le_bytes());
            match e.downgrade {
                Some(id) => {
                    h.update(&[1]);
                    h.update(&id.stable_id().to_le_bytes());
                }
                None => {
                    h.update(&[0]);
                }
            }
        }
        ChainBinding {
            name,
            digest: *h.finalize().as_bytes(),
        }
    }

    pub fn index(&self, v: Version) -> Result<usize, Error> {
        if v == self.base {
            return Ok(0);
        }
        self.edges
            .iter()
            .position(|e| e.version == v)
            .map(|i| i + 1)
            .ok_or(Error::UnknownVersion(v))
    }
}
