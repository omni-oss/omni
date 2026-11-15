use std::io;
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder};

pub fn archive<W: io::Write>(src_dir: &Path, dst: W) -> io::Result<()> {
    // Wrap with gzip encoder
    let enc = GzEncoder::new(dst, Compression::default());
    // Create tar builder on top of gzip
    let mut tar = Builder::new(enc);

    // Append directory *contents* into the archive with root prefix "." so extraction will restore contents into target dir.
    // If you want the directory itself to be included as the top-level entry, change the prefix.
    tar.append_dir_all(".", src_dir)?;

    // Finish writing and ensure gzip is flushed
    let enc = tar.into_inner()?;
    enc.finish()?;
    Ok(())
}

/// Extracts an in-memory archive into `dst_dir`, refusing anything that is not a plain
/// file or directory with a confined relative path.
///
/// `tar` keeps extraction from writing *through* a symlink, but it still creates
/// a symlink or hard-link entry with any target. Restore later follows such a
/// link when it copies or links the cached file, so a hostile archive could read
/// or write outside the workspace. The whole archive is therefore checked before
/// anything is written; a rejected archive leaves `dst_dir` untouched and is
/// reported as [`io::ErrorKind::InvalidData`].
pub fn unarchive_bytes(dst_dir: &Path, bytes: &[u8]) -> io::Result<()> {
    // The bytes are in memory, so any failure while reading them means the data
    // itself is bad (corrupt gzip, truncated tar, a rejected entry).
    validate_entries(bytes).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            e
        } else {
            io::Error::new(io::ErrorKind::InvalidData, e)
        }
    })?;

    let mut archive = Archive::new(GzDecoder::new(bytes));
    // Unpack into dst_dir (will create directories as needed)
    archive.unpack(dst_dir)?;
    Ok(())
}

fn validate_entries(bytes: &[u8]) -> io::Result<()> {
    use tar::EntryType;

    let reject = |why: String| io::Error::new(io::ErrorKind::InvalidData, why);

    let mut archive = Archive::new(GzDecoder::new(bytes));
    for entry in archive.entries()? {
        let entry = entry?;
        let kind = entry.header().entry_type();
        let path = entry.path()?;
        if !matches!(kind, EntryType::Regular | EntryType::Directory) {
            return Err(reject(format!(
                "cache archive entry {path:?} has unsupported type {kind:?}"
            )));
        }
        // The archive root is `.`; every other entry must stay under it.
        if path.as_ref() != Path::new(".")
            && path.as_ref() != Path::new("./")
            && !omni_types::is_confined_relative(&path)
        {
            return Err(reject(format!(
                "cache archive entry {path:?} is not a confined relative path"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::{TempDir, tempdir};

    use super::*;

    fn gz(
        build: impl FnOnce(&mut Builder<GzEncoder<&mut Vec<u8>>>),
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut tar =
            Builder::new(GzEncoder::new(&mut out, Compression::default()));
        build(&mut tar);
        tar.into_inner().unwrap().finish().unwrap();
        out
    }

    fn raw_entry(
        tar: &mut Builder<GzEncoder<&mut Vec<u8>>>,
        name: &[u8],
        kind: tar::EntryType,
        link: Option<&[u8]>,
        data: &[u8],
    ) {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        if let Some(link) = link {
            header.as_old_mut().linkname[..link.len()].copy_from_slice(link);
        }
        header.set_entry_type(kind);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, data).unwrap();
    }

    #[test]
    fn links_and_escaping_names_are_rejected_before_anything_is_written() {
        use tar::EntryType::{Link, Regular, Symlink};

        let hostile: Vec<(&str, Vec<u8>)> = vec![
            (
                "symlink",
                gz(|t| raw_entry(t, b"x.cache", Symlink, Some(b"/etc"), b"")),
            ),
            (
                "hard link",
                gz(|t| {
                    raw_entry(t, b"x.cache", Link, Some(b"/etc/passwd"), b"")
                }),
            ),
            (
                "dot-dot name",
                gz(|t| raw_entry(t, b"../escape", Regular, None, b"x")),
            ),
            (
                "absolute name",
                gz(|t| raw_entry(t, b"/tmp/escape", Regular, None, b"x")),
            ),
            (
                "a good file followed by a symlink",
                gz(|t| {
                    raw_entry(t, b"good.cache", Regular, None, b"ok");
                    raw_entry(t, b"bad.cache", Symlink, Some(b"/etc"), b"");
                }),
            ),
        ];
        for (name, archive) in hostile {
            let dst = tempdir().unwrap();
            let err = unarchive_bytes(dst.path(), &archive)
                .expect_err(&format!("{name} must be rejected"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{name}: {err}");
            assert_eq!(
                std::fs::read_dir(dst.path()).unwrap().count(),
                0,
                "{name}: nothing may be written, not even the earlier good entry"
            );
        }
    }

    #[test]
    fn a_corrupt_archive_is_invalid_data_and_writes_nothing() {
        let dst = tempdir().unwrap();
        let err =
            unarchive_bytes(dst.path(), b"definitely not gzip").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    fn prepare_fixture(dir: &TempDir) {
        let path = dir.path();

        std::fs::create_dir_all(path.join("a")).expect("failed to create dir");
        std::fs::create_dir_all(path.join("b")).expect("failed to create dir");
        std::fs::create_dir_all(path.join("c")).expect("failed to create dir");

        std::fs::write(path.join("a/a.txt"), "a")
            .expect("failed to write file");
        std::fs::write(path.join("b/b.txt"), "b")
            .expect("failed to write file");
        std::fs::write(path.join("c/c.txt"), "c")
            .expect("failed to write file");
    }

    #[test]
    fn test_archive() {
        let dir = tempdir().expect("failed to create tempdir");
        prepare_fixture(&dir);
        let path = dir.path();

        let mut buff = Cursor::new(Vec::new());
        archive(path, &mut buff).expect("failed to archive");

        let buff = buff.into_inner();
        assert!(!buff.is_empty(), "should have data");
    }

    #[test]
    fn test_unarchive() {
        let dir = tempdir().expect("failed to create tempdir");
        prepare_fixture(&dir);
        let path = dir.path();

        let mut buff = Cursor::new(Vec::new());
        archive(path, &mut buff).expect("failed to archive");

        unarchive_bytes(path, &buff.into_inner()).expect("failed to unarchive");

        assert!(
            std::fs::read_to_string(path.join("a/a.txt"))
                .expect("failed to read file")
                .contains("a"),
            "a.txt should exist"
        );
        assert!(
            std::fs::read_to_string(path.join("b/b.txt"))
                .expect("failed to read file")
                .contains("b"),
            "b.txt should exist"
        );
        assert!(
            std::fs::read_to_string(path.join("c/c.txt"))
                .expect("failed to read file")
                .contains("c"),
            "c.txt should exist"
        );
    }
}
