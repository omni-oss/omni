use std::{
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use system_traits::{
    FsRemoveFileAsync as _, FsRenameAsync as _, FsWriteAsync as _,
    impls::RealSys,
};
use tokio::io::AsyncReadExt as _;

/// Writes `bytes` to a sibling temp file and renames it over `path`, so a
/// crash never leaves a torn file and readers see either the old or new
/// content.
pub async fn write_atomic(
    sys: &RealSys,
    path: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    let tmp_path = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let result = async {
        sys.fs_write_async(&tmp_path, bytes).await?;
        sys.fs_rename_async(&tmp_path, path).await
    }
    .await;
    if result.is_err() {
        let _ = sys.fs_remove_file_async(&tmp_path).await;
    }
    result
}

/// Reads a file, failing with [`io::ErrorKind::InvalidData`] when it is larger
/// than `max_len` bytes, without reading more than `max_len + 1` bytes.
pub async fn read_bounded(path: &Path, max_len: u64) -> io::Result<Vec<u8>> {
    // `system_traits` has no streaming open, so this cannot take a `Sys` bound
    // yet. It only ever reads a bounded prefix of a regular file.
    let file = tokio::fs::File::open(path).await?;
    let mut buf = Vec::new();
    file.take(max_len.saturating_add(1))
        .read_to_end(&mut buf)
        .await?;
    if buf.len() as u64 > max_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} exceeds {max_len} bytes", path.display()),
        ));
    }
    Ok(buf)
}
