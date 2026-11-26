use std::io;

use omni_path_utils::{WorkspacePath, WorkspacePathError};

use crate::auth::DenyReason;

omni_codes::prefixes!("input_view");

/// Why the view refused to open a path although the policy may allow it. This
/// is the primitive's result; [`InputError`] adds the path and the diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentViolation {
    /// A symlink was found on the path (Unix).
    Symlink,
    /// The path resolved outside the workspace root.
    EscapesRoot,
    /// The path crosses a mount point inside the root (`RESOLVE_NO_XDEV`).
    MountCrossing,
    /// A reparse point (symlink, junction, mount point) was found (Windows).
    ReparsePoint,
    /// The platform cannot contain reads beneath the root.
    Unsupported,
}

/// Why a name is refused (D9). The same rule applies on every OS, so a
/// discovery run does not accept on Linux a path that fails on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameProblem {
    #[error("contains `:` (a Windows alternate data stream)")]
    Stream,
    #[error("is a reserved Windows device name")]
    ReservedDeviceName,
    #[error("ends with a dot or a space")]
    TrailingDotOrSpace,
    #[error(
        "cannot be a workspace path (not valid UTF-8, or it has a backslash \
         or a drive prefix)"
    )]
    NotRepresentable,
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

/// Every failure carries the workspace-relative path it is about, so a user
/// can tell which file was refused and why. The paths are host-side
/// information: a plugin-facing adapter decides what to pass on.
#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
pub enum InputError {
    #[error("invalid path: {0}")]
    #[diagnostic(
        code = "input_view.invalid_path",
        hint = "use a normalized path relative to the workspace root, with `/` separators and no `.` or `..` segments"
    )]
    InvalidPath(#[from] WorkspacePathError),

    #[error("reading `{}` is denied by policy", .0.path)]
    #[diagnostic(
        code = "input_view.denied",
        hint = "the workspace capability policy does not allow this read; allow it in the policy if it is intended"
    )]
    Denied(DenyReason),

    #[error("the read policy could not be evaluated")]
    #[diagnostic(
        code = "input_view.policy_unavailable",
        hint = "a policy that cannot be evaluated is treated as a denial; fix the capability policy"
    )]
    PolicyUnavailable,

    #[error("`{path}` is, or passes through, a symlink")]
    #[diagnostic(
        code = "input_view.symlink",
        hint = "symlinks are rejected by default; replace it with the real file or directory"
    )]
    Symlink { path: WorkspacePath },

    #[error("`{path}` resolves outside the workspace root")]
    #[diagnostic(
        code = "input_view.escapes_root",
        hint = "inputs must stay inside the workspace root"
    )]
    EscapesRoot { path: WorkspacePath },

    #[error("`{path}` crosses a mount point")]
    #[diagnostic(
        code = "input_view.mount_crossing",
        hint = "inputs must be on the same filesystem as the workspace root (this also covers a directory renamed while it was being read)"
    )]
    MountCrossing { path: WorkspacePath },

    #[error("`{path}` is, or passes through, a reparse point")]
    #[diagnostic(
        code = "input_view.reparse_point",
        hint = "symlinks, junctions and mount points are rejected; replace it with the real file or directory"
    )]
    ReparsePoint { path: WorkspacePath },

    #[error("reading beneath the workspace root is not supported here")]
    #[diagnostic(
        code = "input_view.containment_unsupported",
        audience = environment,
        hint = "this needs Linux 5.6 or newer with `openat2` allowed (not blocked by a sandbox or seccomp filter) or Windows; macOS is not supported yet"
    )]
    ContainmentUnsupported,

    #[error("`{path}` is not supported: a name in it {problem}")]
    #[diagnostic(
        code = "input_view.unsupported_name",
        hint = "rename it or keep it out of the directories omni reads; these names are rejected on every OS so results do not depend on the OS"
    )]
    UnsupportedName { path: String, problem: NameProblem },

    #[error("`{path}` does not match the spelling on disk")]
    #[diagnostic(
        code = "input_view.case_mismatch",
        hint = "use the exact spelling of the file name, including its case"
    )]
    CaseMismatch { path: WorkspacePath },

    #[error("following symlinks is not supported")]
    #[diagnostic(
        code = "input_view.symlink_policy_unsupported",
        hint = "only the default symlink policy (reject) is implemented"
    )]
    SymlinkPolicyUnsupported,

    #[error("`{path}` is not a regular file ({kind:?})")]
    #[diagnostic(
        code = "input_view.not_regular",
        hint = "only regular files can be read"
    )]
    NotRegular {
        path: WorkspacePath,
        kind: EntryKind,
    },

    #[error("`{path}` is not a directory ({kind:?})")]
    #[diagnostic(
        code = "input_view.not_directory",
        hint = "only directories can be listed"
    )]
    NotDirectory {
        path: WorkspacePath,
        kind: EntryKind,
    },

    #[error("`{path}` is {len} bytes, the limit is {limit}")]
    #[diagnostic(
        code = "input_view.too_large",
        hint = "the limit is set by the host; a file this large is not read"
    )]
    TooLarge {
        path: WorkspacePath,
        len: u64,
        limit: u64,
    },

    #[error("input limit exceeded ({0:?})")]
    #[diagnostic(
        code = "input_view.budget_exceeded",
        hint = "this pass read more than the host allows; the limits are host-owned"
    )]
    Budget(BudgetKind),

    #[error("i/o error on `{path}`: {kind}")]
    #[diagnostic(
        code = "input_view.io",
        audience = environment,
        hint = "check that the path is readable and that nothing else is changing it"
    )]
    Io {
        path: WorkspacePath,
        kind: io::ErrorKind,
    },

    #[error("cancelled")]
    #[diagnostic(skip)]
    Cancelled,
}

impl InputError {
    /// The containment failure this error reports, if it is one.
    pub fn containment(&self) -> Option<ContainmentViolation> {
        Some(match self {
            Self::Symlink { .. } => ContainmentViolation::Symlink,
            Self::EscapesRoot { .. } => ContainmentViolation::EscapesRoot,
            Self::MountCrossing { .. } => ContainmentViolation::MountCrossing,
            Self::ReparsePoint { .. } => ContainmentViolation::ReparsePoint,
            Self::ContainmentUnsupported => ContainmentViolation::Unsupported,
            _ => return None,
        })
    }

    pub(crate) fn violation(
        violation: ContainmentViolation,
        path: &WorkspacePath,
    ) -> Self {
        let path = path.clone();
        match violation {
            ContainmentViolation::Symlink => Self::Symlink { path },
            ContainmentViolation::EscapesRoot => Self::EscapesRoot { path },
            ContainmentViolation::MountCrossing => Self::MountCrossing { path },
            ContainmentViolation::ReparsePoint => Self::ReparsePoint { path },
            ContainmentViolation::Unsupported => Self::ContainmentUnsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use omni_diagnostic::Diagnostic as _;

    use super::*;

    fn wp(path: &str) -> WorkspacePath {
        WorkspacePath::parse(path).unwrap()
    }

    const PATH: &str = "zz/distinct/path";

    fn every_variant() -> Vec<InputError> {
        vec![
            InputError::InvalidPath(WorkspacePathError::Empty),
            InputError::Denied(DenyReason {
                path: wp(PATH),
                cause: crate::auth::DenyCause::NoMatch,
            }),
            InputError::PolicyUnavailable,
            InputError::Symlink { path: wp(PATH) },
            InputError::EscapesRoot { path: wp(PATH) },
            InputError::MountCrossing { path: wp(PATH) },
            InputError::ReparsePoint { path: wp(PATH) },
            InputError::ContainmentUnsupported,
            InputError::UnsupportedName {
                path: PATH.into(),
                problem: NameProblem::Stream,
            },
            InputError::CaseMismatch { path: wp(PATH) },
            InputError::SymlinkPolicyUnsupported,
            InputError::NotRegular {
                path: wp(PATH),
                kind: EntryKind::Dir,
            },
            InputError::NotDirectory {
                path: wp(PATH),
                kind: EntryKind::File,
            },
            InputError::TooLarge {
                path: wp(PATH),
                len: 2,
                limit: 1,
            },
            InputError::Budget(BudgetKind::Operations),
            InputError::Io {
                path: wp(PATH),
                kind: io::ErrorKind::PermissionDenied,
            },
        ]
    }

    #[test]
    fn every_failure_has_a_stable_code_and_a_hint() {
        let mut codes = Vec::new();
        for error in every_variant() {
            assert!(error.reportable(), "{error}");
            assert!(error.code().starts_with("input_view."), "{error}");
            assert!(!error.hints().is_empty(), "{error}");
            // Every failure that is about a path says which one.
            let about_a_path = !matches!(
                error,
                InputError::InvalidPath(_)
                    | InputError::PolicyUnavailable
                    | InputError::ContainmentUnsupported
                    | InputError::SymlinkPolicyUnsupported
                    | InputError::Budget(_)
            );
            if about_a_path {
                assert!(error.to_string().contains(PATH), "{error}");
            }
            // The code is registered, so the CLI can look it up.
            assert!(omni_codes::get(error.code()).is_some(), "{error}");
            codes.push(error.code());
        }
        let unique: std::collections::BTreeSet<_> =
            codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len(), "codes must be unique");
    }

    #[test]
    fn the_registered_codes_are_valid() {
        omni_codes::validate().unwrap();
    }

    #[test]
    fn cancellation_is_not_a_reportable_failure() {
        assert!(!InputError::Cancelled.reportable());
    }

    #[test]
    fn the_message_says_which_path_and_why() {
        let error = InputError::UnsupportedName {
            path: "docs/a:b".into(),
            problem: NameProblem::Stream,
        };
        let message = error.to_string();
        assert!(message.contains("docs/a:b"), "{message}");
        assert!(message.contains("alternate data stream"), "{message}");
        let report = eyre::Report::new(error);
        let found = omni_diagnostic::find(&report);
        assert_eq!(
            found.map(|diagnostic| diagnostic.code()),
            Some("input_view.unsupported_name")
        );
    }

    #[test]
    fn containment_errors_name_the_path_and_map_back() {
        let error =
            InputError::violation(ContainmentViolation::Symlink, &wp("a/b"));
        assert!(error.to_string().contains("a/b"));
        assert_eq!(error.containment(), Some(ContainmentViolation::Symlink));
        assert_eq!(InputError::Cancelled.containment(), None);
    }
}
