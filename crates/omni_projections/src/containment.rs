use std::path::Path;

use system_traits::FsCanonicalizeAsync;

use crate::error::Result;

/// Whether a source entry's real (symlink-followed) path stays within the
/// source root. Rejects `..`/symlink escapes that a lexical check cannot see.
///
/// Both paths are canonicalized, so the source root must exist on disk.
pub async fn source_realpath_contained<S>(
    sys: &S,
    source_root: &Path,
    source_abs: &Path,
) -> Result<bool>
where
    S: FsCanonicalizeAsync + Sync,
{
    let real_root = sys.fs_canonicalize_async(source_root).await?;
    let real_entry = sys.fs_canonicalize_async(source_abs).await?;
    Ok(real_entry.starts_with(&real_root))
}
