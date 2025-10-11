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

pub fn unarchive<R: io::Read>(dst_dir: &Path, src: R) -> io::Result<()> {
    // Wrap with gzip decoder
    let decoder = GzDecoder::new(src);
    // Create tar archive reader
    let mut archive = Archive::new(decoder);

    // Unpack into dst_dir (will create directories as needed)
    archive.unpack(dst_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::{TempDir, tempdir};

    use super::*;

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
        archive(&path, &mut buff).expect("failed to archive");

        let buff = buff.into_inner();
        assert!(!buff.is_empty(), "should have data");
    }

    #[test]
    fn test_unarchive() {
        let dir = tempdir().expect("failed to create tempdir");
        prepare_fixture(&dir);
        let path = dir.path();

        let mut buff = Cursor::new(Vec::new());
        archive(&path, &mut buff).expect("failed to archive");

        let mut buff2 = Cursor::new(buff.into_inner());
        unarchive(&path, &mut buff2).expect("failed to unarchive");

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
