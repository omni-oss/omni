/// Only recover from errors that can prevent hardlinks while still allowing a
/// copy. Missing sources, existing destinations and other failures propagate.
pub fn should_copy_after_hardlink_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::CrossesDevices
            | std::io::ErrorKind::Unsupported
            | std::io::ErrorKind::PermissionDenied
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn copy_fallback_is_limited_to_filesystem_restrictions() {
        use std::io::{Error, ErrorKind};

        for kind in [
            ErrorKind::CrossesDevices,
            ErrorKind::Unsupported,
            ErrorKind::PermissionDenied,
        ] {
            assert!(super::should_copy_after_hardlink_error(&Error::from(
                kind
            )));
        }
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::AlreadyExists,
            ErrorKind::InvalidInput,
            ErrorKind::StorageFull,
            ErrorKind::Other,
        ] {
            assert!(!super::should_copy_after_hardlink_error(&Error::from(
                kind
            )));
        }
    }
}
