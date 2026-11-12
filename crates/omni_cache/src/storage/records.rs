use std::{
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::io::AsyncReadExt as _;

/// Writes `bytes` to a sibling temp file and renames it over `path`, so a
/// crash never leaves a torn file and readers see either the old or new
/// content.
pub async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp_path = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    tokio::fs::write(&tmp_path, bytes).await?;
    tokio::fs::rename(&tmp_path, path).await?;
    Ok(())
}

/// Reads a file, failing with [`io::ErrorKind::InvalidData`] when it is larger
/// than `max_len` bytes, without reading more than `max_len + 1` bytes.
pub async fn read_bounded(path: &Path, max_len: u64) -> io::Result<Vec<u8>> {
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
