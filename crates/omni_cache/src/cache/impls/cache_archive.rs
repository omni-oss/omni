use std::io::{self, Read};
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
    unarchive_bytes_limited(dst_dir, bytes, MAX_ENTRIES, MAX_EXPANDED_BYTES)
}

/// An archive is capped on its compressed size at download, but a tiny gzip
/// stream can expand enormously. Both payload and the entire expanded tar stream
/// (including headers, extensions, padding and trailing data) are bounded.
const MAX_ENTRIES: u64 = 200_000;
const MAX_EXPANDED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
// Large enough for GNU long paths emitted by archive(), without allowing tar's
// internal read_all of extension records to allocate attacker-selected gigabytes.
const MAX_EXTENSION_BYTES: u64 = 1024 * 1024;

fn invalid_archive(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Unlike Read::take, exceeding the limit is an error, not a synthetic EOF.
struct ExpandedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for ExpandedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return match self.inner.read(&mut [0])? {
                0 => Ok(0),
                _ => Err(invalid_archive(
                    "cache archive expanded stream is too large",
                )),
            };
        }
        let len = self.remaining.min(buf.len() as u64) as usize;
        let n = self.inner.read(&mut buf[..len])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

fn expanded_reader(
    bytes: &[u8],
    limit: u64,
) -> ExpandedReader<GzDecoder<&[u8]>> {
    ExpandedReader {
        inner: GzDecoder::new(bytes),
        remaining: limit,
    }
}

fn skip_exact(reader: &mut impl Read, size: u64) -> io::Result<()> {
    if io::copy(&mut reader.take(size), &mut io::sink())? != size {
        return Err(invalid_archive("truncated cache archive entry"));
    }
    Ok(())
}

/// Read only raw headers until extension lengths have been checked. Restarting
/// the raw iterator at each header lets us skip the *effective* PAX size rather
/// than letting raw(true) follow the unoverridden header size into file data.
fn preflight(
    bytes: &[u8],
    max_entries: u64,
    max_expanded: u64,
) -> io::Result<()> {
    let mut reader = expanded_reader(bytes, max_expanded);
    let mut count = 0u64;
    let mut pax_size = None;
    loop {
        let mut archive = Archive::new(&mut reader);
        let mut entries = archive.entries()?.raw(true);
        let Some(entry) = entries.next() else { break };
        let mut entry = entry?;
        count = count.saturating_add(1);
        // A logical entry may have GNU longname, longlink and local PAX records.
        if count > max_entries.saturating_mul(4) {
            return Err(invalid_archive(
                "cache archive has too many raw entries",
            ));
        }
        let header = entry.header();
        let kind = header.entry_type();
        let recognized =
            header.as_gnu().is_some() || header.as_ustar().is_some();
        let extension = kind.is_gnu_longname()
            || kind.is_gnu_longlink()
            || kind.is_pax_local_extensions()
            || kind.is_pax_global_extensions();
        let mut size = entry.size();
        if extension {
            if size > MAX_EXTENSION_BYTES {
                return Err(invalid_archive(
                    "cache archive extension is too large",
                ));
            }
        } else if !kind.is_file() && !kind.is_dir() {
            // In particular, do not let the semantic parser allocate GNU sparse
            // maps (or expand holes) before it returns an Entry for validation.
            return Err(invalid_archive(
                "cache archive has unsupported entry type",
            ));
        }
        if recognized && kind.is_pax_local_extensions() {
            let mut metadata = Vec::new();
            entry.read_to_end(&mut metadata)?;
            pax_size = None;
            for field in tar::PaxExtensions::new(&metadata) {
                let field = field?;
                if field.key_bytes() == b"size" && pax_size.is_none() {
                    pax_size = Some(
                        field
                            .value()
                            .ok()
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or_else(|| {
                                invalid_archive("invalid PAX size")
                            })?,
                    );
                }
            }
            // The entry body was consumed above; only its padding remains.
            size = (512 - size % 512) % 512;
        } else {
            if !extension {
                size = pax_size.take().unwrap_or(size);
            }
            if size > max_expanded {
                return Err(invalid_archive(
                    "cache archive entry is too large",
                ));
            }
            size = size.checked_add(511).ok_or_else(|| {
                invalid_archive("cache archive size overflow")
            })? / 512
                * 512;
        }
        drop(entry);
        skip_exact(&mut reader, size)?;
    }
    // Tar stops at its end marker. Still charge all trailing expanded bytes and
    // verify the gzip checksum before any destination writes.
    io::copy(&mut reader, &mut io::sink())?;
    Ok(())
}

fn unarchive_bytes_limited(
    dst_dir: &Path,
    bytes: &[u8],
    max_entries: u64,
    max_expanded: u64,
) -> io::Result<()> {
    // The bytes are in memory, so any failure while reading them means the data
    // itself is bad (corrupt gzip, truncated tar, a rejected entry).
    preflight(bytes, max_entries, max_expanded)
        .and_then(|()| validate_entries(bytes, max_entries, max_expanded))
        .map_err(|e| {
            if e.kind() == io::ErrorKind::InvalidData {
                e
            } else {
                io::Error::new(io::ErrorKind::InvalidData, e)
            }
        })?;

    std::fs::create_dir_all(dst_dir)?;
    let dst_dir = &dst_dir.canonicalize()?;
    let mut archive = Archive::new(expanded_reader(bytes, max_expanded));
    let (mut count, mut total) = (0u64, 0u64);
    // Match Archive::unpack: defer directory permissions until descendants have
    // been written, applying deeper directories first.
    let mut directories = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        check_entry_limits(
            &entry,
            &mut count,
            &mut total,
            max_entries,
            max_expanded,
        )?;
        if entry.header().entry_type().is_dir() {
            directories.push(entry);
        } else {
            entry.unpack_in(dst_dir)?;
        }
    }
    directories.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut directory in directories {
        directory.unpack_in(dst_dir)?;
    }
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    Ok(())
}

fn check_entry_limits<R: Read>(
    entry: &tar::Entry<'_, R>,
    count: &mut u64,
    total: &mut u64,
    max_entries: u64,
    max_expanded: u64,
) -> io::Result<()> {
    *count = count.saturating_add(1);
    *total = total.checked_add(entry.size()).ok_or_else(|| {
        invalid_archive("cache archive payload size overflow")
    })?;
    if *count > max_entries || *total > max_expanded {
        return Err(invalid_archive(
            "cache archive expanded payload is too large",
        ));
    }
    Ok(())
}

fn validate_entries(
    bytes: &[u8],
    max_entries: u64,
    max_expanded: u64,
) -> io::Result<()> {
    use tar::EntryType;

    let reject = |why: String| io::Error::new(io::ErrorKind::InvalidData, why);

    let mut archive = Archive::new(expanded_reader(bytes, max_expanded));
    let (mut count, mut total) = (0u64, 0u64);
    for entry in archive.entries()? {
        let entry = entry?;
        check_entry_limits(
            &entry,
            &mut count,
            &mut total,
            max_entries,
            max_expanded,
        )?;
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
    fn an_archive_that_expands_past_the_limits_is_rejected_unwritten() {
        let many = gz(|t| {
            for i in 0..5 {
                raw_entry(
                    t,
                    format!("f{i}").as_bytes(),
                    tar::EntryType::Regular,
                    None,
                    b"x",
                );
            }
        });
        let big = gz(|t| {
            raw_entry(t, b"big", tar::EntryType::Regular, None, &[0u8; 4096])
        });
        for (name, bytes, entries, expanded) in
            [("count", many, 4, u64::MAX), ("size", big, u64::MAX, 1000)]
        {
            let dir = tempdir().unwrap();
            let err =
                unarchive_bytes_limited(dir.path(), &bytes, entries, expanded)
                    .expect_err(name);
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{name}");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }

    fn pax_override_archive() -> Vec<u8> {
        gz(|t| {
            t.append_pax_extensions([("size", b"4096".as_slice())])
                .unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_path("pax-file").unwrap();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(0);
            header.set_cksum();
            // Builder writes all data even though the header declares zero.
            t.append(&header, &[b'x'; 4096][..]).unwrap();
        })
    }

    #[test]
    fn effective_pax_size_is_limited_not_the_header_size() {
        let bytes = pax_override_archive();
        let err = validate_entries(&bytes, MAX_ENTRIES, 2048).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("payload"), "{err}");
        let dst = tempdir().unwrap();
        assert!(
            unarchive_bytes_limited(dst.path(), &bytes, MAX_ENTRIES, 2048)
                .is_err()
        );
        assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    #[test]
    fn bounded_pax_size_override_round_trips() {
        let dst = tempdir().unwrap();
        unarchive_bytes(dst.path(), &pax_override_archive()).unwrap();
        assert_eq!(
            std::fs::read(dst.path().join("pax-file")).unwrap(),
            vec![b'x'; 4096]
        );
    }

    #[test]
    fn pax_size_cannot_hide_an_oversized_extension_from_preflight() {
        let bytes = gz(|t| {
            t.append_pax_extensions([("size", b"0".as_slice())])
                .unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_path("empty").unwrap();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(4096);
            header.set_cksum();
            t.append(&header, io::empty()).unwrap();
            header.set_entry_type(tar::EntryType::GNULongName);
            header.set_size(MAX_EXTENSION_BYTES + 1);
            header.set_cksum();
            t.append(&header, io::empty()).unwrap();
        });
        let dst = tempdir().unwrap();
        let err = unarchive_bytes(dst.path(), &bytes).unwrap_err();
        assert!(err.to_string().contains("extension is too large"), "{err}");
        assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    #[test]
    fn pending_pax_size_applies_to_the_file_not_intermediate_extensions() {
        let bytes = gz(|t| {
            t.append_pax_extensions([("size", b"4".as_slice())])
                .unwrap();
            raw_entry(
                t,
                b"name",
                tar::EntryType::GNULongName,
                None,
                b"long-name\0",
            );
            raw_entry(t, b"short", tar::EntryType::Regular, None, b"data");
        });
        let dst = tempdir().unwrap();
        unarchive_bytes(dst.path(), &bytes).unwrap();
        assert_eq!(
            std::fs::read(dst.path().join("long-name")).unwrap(),
            b"data"
        );
    }

    #[test]
    fn exact_logical_entry_limit_accepts_gnu_extension_records() {
        let bytes = gz(|t| {
            raw_entry(
                t,
                b"longname",
                tar::EntryType::GNULongName,
                None,
                b"long-file\0",
            );
            raw_entry(t, b"f", tar::EntryType::Regular, None, b"x");
        });
        let dst = tempdir().unwrap();
        unarchive_bytes_limited(dst.path(), &bytes, 1, MAX_EXPANDED_BYTES)
            .unwrap();
        assert_eq!(std::fs::read(dst.path().join("long-file")).unwrap(), b"x");
    }

    #[test]
    fn corrupt_gzip_checksum_is_rejected_before_writes() {
        let mut bytes =
            gz(|t| raw_entry(t, b"f", tar::EntryType::Regular, None, b"x"));
        let checksum = bytes.len() - 8;
        bytes[checksum] ^= 1;
        let dst = tempdir().unwrap();
        let err = unarchive_bytes(dst.path(), &bytes).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    #[test]
    fn oversized_extensions_are_rejected_before_semantic_parsing() {
        use tar::EntryType::{
            GNULongLink, GNULongName, XGlobalHeader, XHeader,
        };
        for kind in [GNULongName, GNULongLink, XHeader, XGlobalHeader] {
            let bytes = gz(|t| {
                raw_entry(t, b"good", tar::EntryType::Regular, None, b"ok");
                // Only a header is needed: reject before trying to read or
                // allocate its declared metadata body.
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(kind);
                header.set_size(MAX_EXTENSION_BYTES + 1);
                header.set_cksum();
                t.append(&header, io::empty()).unwrap();
            });
            let dst = tempdir().unwrap();
            let err = unarchive_bytes(dst.path(), &bytes).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(
                err.to_string().contains("extension is too large"),
                "{kind:?}: {err}"
            );
            assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn raw_stream_limit_includes_padding_metadata_and_trailing_data() {
        let cases = [
            gz(|t| raw_entry(t, b"f", tar::EntryType::Regular, None, b"x")),
            gz(|t| {
                t.append_pax_extensions([("comment", &[b'x'; 1024][..])])
                    .unwrap();
                raw_entry(t, b"f", tar::EntryType::Regular, None, b"");
            }),
            {
                let mut encoder =
                    GzEncoder::new(Vec::new(), Compression::default());
                io::Write::write_all(&mut encoder, &[0; 4096]).unwrap();
                encoder.finish().unwrap()
            },
        ];
        for bytes in cases {
            let dst = tempdir().unwrap();
            let err =
                unarchive_bytes_limited(dst.path(), &bytes, MAX_ENTRIES, 1024)
                    .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("expanded stream"), "{err}");
            assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn expanded_reader_rejects_overrun_but_accepts_exact_limit() {
        for (data, limit, accepted) in
            [(b"abc".as_slice(), 3, true), (b"abcd", 3, false)]
        {
            let mut reader = ExpandedReader {
                inner: data,
                remaining: limit,
            };
            let result = io::copy(&mut reader, &mut io::sink());
            assert_eq!(result.is_ok(), accepted);
            if let Err(err) = result {
                assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            }
        }
    }

    #[test]
    fn sparse_entries_are_rejected_before_sparse_map_parsing() {
        let bytes = gz(|t| {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::GNUSparse);
            header.as_gnu_mut().unwrap().isextended[0] = 1;
            header.set_size(0);
            header.set_cksum();
            t.append(&header, io::empty()).unwrap();
        });
        let dst = tempdir().unwrap();
        let err = unarchive_bytes(dst.path(), &bytes).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unsupported entry type"), "{err}");
        assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    #[test]
    fn archive_generated_gnu_long_paths_round_trip() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        let relative =
            Path::new(&"a".repeat(90)).join("b".repeat(90)).join("file");
        std::fs::create_dir_all(src.path().join(&relative).parent().unwrap())
            .unwrap();
        std::fs::write(src.path().join(&relative), b"long path payload")
            .unwrap();
        let mut bytes = Vec::new();
        archive(src.path(), &mut bytes).unwrap();
        unarchive_bytes(dst.path(), &bytes).unwrap();
        assert_eq!(
            std::fs::read(dst.path().join(relative)).unwrap(),
            b"long path payload"
        );
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
