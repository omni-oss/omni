use std::path::Path;

use system_traits::{FsMetadataAsync as _, impls::RealSys};

/// `try_exists` semantics: follows symlinks, so a dangling link is absent.
///
/// `system_traits`' `fs_exists_async` is built on `symlink_metadata`, which
/// does **not** follow links and reports a dangling link as present. Use this
/// helper wherever `try_exists` semantics are wanted.
pub(super) async fn try_exists_following(
    sys: &RealSys,
    path: impl AsRef<Path> + Send,
) -> std::io::Result<bool> {
    match sys.fs_metadata_async(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

mod cache_archive;
mod hybrid;
mod last_used_db;
mod origin;
mod partition;
mod remote_scope;
mod signing_policy;
mod verify;

pub use hybrid::*;
pub use signing_policy::*;
