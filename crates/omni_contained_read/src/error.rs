use std::io;

use omni_diagnostic::Diagnostic as _;
use omni_input_view::InputError;
use omni_path_utils::WorkspacePathError;

/// The `io::Error` a script sees for a failed contained read.
///
/// The message is `"<code>: <message>"` with the stable diagnostic code and the
/// workspace-relative path the error is about, never the absolute host path.
/// The full diagnostic (with its hint) is logged host-side.
pub fn to_io_error(error: InputError) -> io::Error {
    use InputError as E;
    use io::ErrorKind as K;

    if matches!(error, E::Cancelled) {
        return io::Error::new(K::Interrupted, "input_view: read cancelled");
    }
    let kind = match &error {
        E::Denied(_)
        | E::PolicyUnavailable
        | E::Symlink { .. }
        | E::EscapesRoot { .. }
        | E::MountCrossing { .. }
        | E::ReparsePoint { .. }
        | E::UnsupportedName { .. }
        | E::CaseMismatch { .. }
        | E::NotRegular { .. } => K::PermissionDenied,
        E::NotDirectory { .. }
        | E::InvalidPath(_)
        | E::TooLarge { .. }
        | E::Budget(_)
        | E::SymlinkPolicyUnsupported => K::InvalidInput,
        E::ContainmentUnsupported => K::Unsupported,
        E::Io { kind, .. } => *kind,
        E::Cancelled => K::Interrupted,
    };
    // `OutsideRoot` embeds the absolute root; routing handles it before an
    // `InvalidPath` is built, and this keeps the root out even if one slips by.
    let message = match &error {
        E::InvalidPath(WorkspacePathError::OutsideRoot { .. }) => {
            format!("{}: the path is not inside the workspace", error.code())
        }
        _ => format!("{}: {error}", error.code()),
    };
    tracing::warn!(code = error.code(), "contained read refused: {error}");
    io::Error::new(kind, message)
}

#[cfg(test)]
mod tests {
    use omni_input_view::{
        BudgetKind, DenyCause, DenyReason, EntryKind, NameProblem,
    };
    use omni_path_utils::{WorkspacePath, WorkspacePathError};

    use super::*;

    fn wp(path: &str) -> WorkspacePath {
        WorkspacePath::parse(path).unwrap()
    }

    const ROOT: &str = "/absolute/host/root";

    #[test]
    fn every_variant_maps_to_the_stated_kind_code_and_message() {
        use io::ErrorKind as K;
        let denied = || {
            InputError::Denied(DenyReason {
                path: wp("a/b"),
                cause: DenyCause::NoMatch,
            })
        };
        let cases: Vec<(InputError, K, &str)> = vec![
            (denied(), K::PermissionDenied, "input_view.denied"),
            (
                InputError::PolicyUnavailable,
                K::PermissionDenied,
                "input_view.policy_unavailable",
            ),
            (
                InputError::Symlink { path: wp("a/b") },
                K::PermissionDenied,
                "input_view.symlink",
            ),
            (
                InputError::EscapesRoot { path: wp("a/b") },
                K::PermissionDenied,
                "input_view.escapes_root",
            ),
            (
                InputError::MountCrossing { path: wp("a/b") },
                K::PermissionDenied,
                "input_view.mount_crossing",
            ),
            (
                InputError::ReparsePoint { path: wp("a/b") },
                K::PermissionDenied,
                "input_view.reparse_point",
            ),
            (
                InputError::UnsupportedName {
                    path: "a/b".into(),
                    problem: NameProblem::Stream,
                },
                K::PermissionDenied,
                "input_view.unsupported_name",
            ),
            (
                InputError::CaseMismatch { path: wp("a/b") },
                K::PermissionDenied,
                "input_view.case_mismatch",
            ),
            (
                InputError::NotRegular {
                    path: wp("a/b"),
                    kind: EntryKind::Dir,
                },
                K::PermissionDenied,
                "input_view.not_regular",
            ),
            (
                InputError::NotDirectory {
                    path: wp("a/b"),
                    kind: EntryKind::File,
                },
                K::InvalidInput,
                "input_view.not_directory",
            ),
            (
                InputError::InvalidPath(WorkspacePathError::Empty),
                K::InvalidInput,
                "input_view.invalid_path",
            ),
            (
                InputError::TooLarge {
                    path: wp("a/b"),
                    len: 2,
                    limit: 1,
                },
                K::InvalidInput,
                "input_view.too_large",
            ),
            (
                InputError::Budget(BudgetKind::Operations),
                K::InvalidInput,
                "input_view.budget_exceeded",
            ),
            (
                InputError::SymlinkPolicyUnsupported,
                K::InvalidInput,
                "input_view.symlink_policy_unsupported",
            ),
            (
                InputError::ContainmentUnsupported,
                K::Unsupported,
                "input_view.containment_unsupported",
            ),
            (
                InputError::Io {
                    path: wp("a/b"),
                    kind: K::WouldBlock,
                },
                K::WouldBlock,
                "input_view.io",
            ),
        ];
        for (error, kind, code) in cases {
            let io = to_io_error(error);
            assert_eq!(io.kind(), kind, "{code}");
            let text = io.to_string();
            assert!(text.starts_with(&format!("{code}: ")), "{text}");
            assert!(!text.contains(ROOT), "{text}");
        }
        let cancelled = to_io_error(InputError::Cancelled);
        assert_eq!(cancelled.kind(), K::Interrupted);
        assert_eq!(cancelled.to_string(), "input_view: read cancelled");
    }

    #[test]
    fn the_outside_root_error_never_shows_the_root() {
        let error = InputError::InvalidPath(WorkspacePathError::OutsideRoot {
            path: "/x".into(),
            root: ROOT.into(),
        });
        let text = to_io_error(error).to_string();
        assert!(text.starts_with("input_view.invalid_path"), "{text}");
        assert!(!text.contains(ROOT), "{text}");
    }
}
