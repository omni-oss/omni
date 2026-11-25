use std::collections::BTreeMap;

use omni_path_utils::WorkspacePath;

use crate::digest::Digest;

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
