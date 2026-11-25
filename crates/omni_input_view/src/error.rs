use std::io;

use omni_path_utils::{WorkspacePath, WorkspacePathError};

use crate::auth::DenyReason;

/// Why the view refused to open a path although the policy may allow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContainmentViolation {
    /// A symlink was found on the path (Unix).
    #[error("path contains a symlink")]
    Symlink,
    /// The path resolved outside the workspace root.
    #[error("path escapes the workspace root")]
    EscapesRoot,
    /// The path crosses a mount point inside the root (`RESOLVE_NO_XDEV`).
    #[error("path crosses a mount point")]
    MountCrossing,
    /// A reparse point (symlink, junction, mount point) was found (Windows).
    #[error("path contains a reparse point")]
    ReparsePoint,
    /// A name is rejected on every OS (D9): stream, reserved device name or
    /// trailing dot or space.
    #[error("path contains a name that is not supported")]
    NameUnsupported,
    /// The platform cannot contain reads beneath the root.
    #[error("beneath-root containment is not supported here")]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    PathDepth,
    TotalBytes,
    Inputs,
    DirEntries,
    TotalDirEntries,
    DirScan,
    Observations,
    Operations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("invalid path: {0}")]
    InvalidPath(#[from] WorkspacePathError),
    #[error("read denied by policy")]
    Denied(DenyReason),
    #[error("policy could not be evaluated")]
    PolicyUnavailable,
    #[error("containment: {0}")]
    Containment(#[from] ContainmentViolation),
    #[error("path `{path}` does not match the on-disk spelling")]
    CaseMismatch { path: WorkspacePath },
    #[error("the symlink policy is not supported")]
    SymlinkPolicyUnsupported,
    #[error("entry is not a regular file ({0:?})")]
    NotRegular(EntryKind),
    #[error("file is {len} bytes, limit is {limit}")]
    TooLarge { len: u64, limit: u64 },
    #[error("budget exceeded ({0:?})")]
    Budget(BudgetKind),
    #[error("i/o error on `{path}`: {kind}")]
    Io {
        path: WorkspacePath,
        kind: io::ErrorKind,
    },
    #[error("cancelled")]
    Cancelled,
}
