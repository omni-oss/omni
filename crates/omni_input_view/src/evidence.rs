use std::collections::BTreeMap;

use omni_path_utils::WorkspacePath;

use crate::{digest::Digest, error::EntryKind};

/// Advisory identity of a file; never a substitute for the content digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileIdentity {
    pub dev: Option<u64>,
    pub ino: Option<u64>,
    pub size: u64,
    pub mtime_ns: Option<i128>,
    pub ctime_ns: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    File {
        len: u64,
        digest: Digest,
        id: Option<FileIdentity>,
    },
    Missing,
    /// `exists` saw this kind; content was not captured.
    Present(EntryKind),
    Dir {
        entry_count: u32,
        membership_digest: Digest,
        id: Option<FileIdentity>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathEvidence {
    pub observations: Vec<Observation>,
    pub conflict: bool,
}

/// In-memory evidence, ordered by path. Populated from F10.2b.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Evidence {
    pub paths: BTreeMap<WorkspacePath, PathEvidence>,
}

impl Observation {
    /// The observed kind, if the path existed.
    pub fn kind(&self) -> Option<EntryKind> {
        match self {
            Self::File { .. } => Some(EntryKind::File),
            Self::Dir { .. } => Some(EntryKind::Dir),
            Self::Present(kind) => Some(*kind),
            Self::Missing => None,
        }
    }

    /// Same fact, ignoring the advisory identity.
    pub(crate) fn same_fact(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::File {
                    len: a, digest: x, ..
                },
                Self::File {
                    len: b, digest: y, ..
                },
            ) => a == b && x == y,
            (
                Self::Dir {
                    entry_count: a,
                    membership_digest: x,
                    ..
                },
                Self::Dir {
                    entry_count: b,
                    membership_digest: y,
                    ..
                },
            ) => a == b && x == y,
            (Self::Present(a), Self::Present(b)) => a == b,
            (Self::Missing, Self::Missing) => true,
            _ => false,
        }
    }

    /// A `Present(kind)` says nothing a `File` or `Dir` of that kind does not,
    /// so the two never conflict.
    pub(crate) fn compatible(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Present(kind), full) | (full, Self::Present(kind)) => {
                full.kind() == Some(*kind)
            }
            _ => false,
        }
    }
}

impl PathEvidence {
    /// Adds an observation. Identical ones collapse; a different one is kept
    /// and marks the path as conflicting unless it is compatible. Returns
    /// `false` when the per-path bound would be exceeded.
    pub(crate) fn record(
        &mut self,
        observation: Observation,
        max_observations: usize,
    ) -> bool {
        if self
            .observations
            .iter()
            .any(|seen| seen.same_fact(&observation))
        {
            return true;
        }
        if self.observations.len() >= max_observations {
            return false;
        }
        if !self
            .observations
            .iter()
            .all(|seen| seen.compatible(&observation))
        {
            self.conflict = true;
        }
        self.observations.push(observation);
        true
    }
}
