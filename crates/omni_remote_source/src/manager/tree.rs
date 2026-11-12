//! A published store tree: its completeness manifest and read-only seal.
//!
//! The manifest lives *beside* the tree (`<commit>.manifest`), never inside it:
//! a repository may track a root file with any name, and the export must be
//! byte-for-byte what the commit contains. It is written after the tree is
//! renamed into place, so a tree without a manifest is treated as incomplete
//! and re-exported. A reader that finds files missing or the wrong size (a
//! cleaner deleted part of the tree) does the same.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    files: Vec<ManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestEntry {
    /// Path relative to the tree root, with `/` separators.
    path: String,
    size: u64,
    symlink: bool,
}

/// Seal the exported tree in `dir`: make its files read-only, so a hard link or
/// file symlink into the tree cannot edit shared content in place, and return
/// the manifest bytes to publish beside it.
///
/// Directories stay writable on purpose: the store must be safe to delete with
/// ordinary tools (`rm -rf`, a cache cleaner). That is also why `auto` never
/// links a *directory* from the store: a directory symlink could add or replace
/// files in the shared tree.
pub(super) fn seal(dir: &Path) -> io::Result<Vec<u8>> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = Manifest { files };
    let bytes = serde_json::to_vec(&manifest).map_err(io::Error::other)?;
    for entry in &manifest.files {
        if !entry.symlink {
            set_readonly(&dir.join(&entry.path), true)?;
        }
    }
    Ok(bytes)
}

fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(readonly);
    fs::set_permissions(path, permissions)
}

/// Whether `dir` is a complete published tree: its manifest exists and every
/// listed file exists with its recorded size.
pub(super) fn is_complete(dir: &Path, manifest: &Path) -> bool {
    let Ok(body) = fs::read(manifest) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&body) else {
        return false;
    };
    manifest.files.iter().all(|entry| {
        let path = dir.join(&entry.path);
        match fs::symlink_metadata(&path) {
            Ok(meta) if entry.symlink => meta.is_symlink(),
            Ok(meta) => meta.is_file() && meta.len() == entry.size,
            Err(_) => false,
        }
    })
}

fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<ManifestEntry>,
) -> io::Result<()> {
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push(ManifestEntry {
                path: relative,
                size: meta.len(),
                symlink: meta.is_symlink(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), b"aaa").unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"bb").unwrap();
        dir
    }

    fn manifest_file(dir: &tempfile::TempDir) -> PathBuf {
        let path = dir.path().join("..").join(format!(
            "{}.manifest",
            dir.path().file_name().unwrap().to_string_lossy()
        ));
        path
    }

    fn publish(dir: &tempfile::TempDir) -> PathBuf {
        let manifest = seal(dir.path()).unwrap();
        let path = manifest_file(dir);
        fs::write(&path, manifest).unwrap();
        path
    }

    #[test]
    fn a_sealed_tree_is_complete_and_read_only() {
        let dir = tree();
        let manifest = manifest_file(&dir);
        assert!(!is_complete(dir.path(), &manifest), "no manifest yet");

        let manifest = publish(&dir);

        assert!(is_complete(dir.path(), &manifest));
        for file in ["a.txt", "sub/b.txt"] {
            let meta = fs::metadata(dir.path().join(file)).unwrap();
            assert!(meta.permissions().readonly(), "{file}");
        }
        let _ = fs::remove_file(manifest);
    }

    #[test]
    fn the_manifest_never_overwrites_a_tracked_dot_manifest_file() {
        let dir = tree();
        fs::write(dir.path().join(".manifest"), b"tracked by the repo")
            .unwrap();
        let manifest = publish(&dir);

        assert_eq!(
            fs::read(dir.path().join(".manifest")).unwrap(),
            b"tracked by the repo"
        );
        assert!(is_complete(dir.path(), &manifest));
        let _ = fs::remove_file(manifest);
    }

    #[cfg(unix)]
    #[test]
    fn a_sealed_tree_refuses_in_place_edits_but_stays_deletable() {
        let dir = tree();
        let manifest = publish(&dir);

        assert!(fs::write(dir.path().join("a.txt"), b"AAA").is_err());
        assert!(fs::write(dir.path().join("sub/b.txt"), b"BB").is_err());

        // Ordinary deletion works without any chmod: the store is safe to
        // delete.
        fs::remove_dir_all(dir.path().join("sub")).unwrap();
        fs::remove_file(dir.path().join("a.txt")).unwrap();
        let _ = fs::remove_file(manifest);
    }

    #[test]
    fn a_missing_or_resized_file_makes_the_tree_incomplete() {
        let dir = tree();
        let manifest = publish(&dir);
        assert!(is_complete(dir.path(), &manifest));

        fs::remove_file(dir.path().join("sub/b.txt")).unwrap();
        assert!(!is_complete(dir.path(), &manifest));

        fs::write(dir.path().join("sub/b.txt"), b"much longer").unwrap();
        assert!(!is_complete(dir.path(), &manifest));
        let _ = fs::remove_file(manifest);
    }

    #[test]
    fn a_corrupt_manifest_is_incomplete_not_an_error() {
        let dir = tree();
        let manifest = publish(&dir);
        fs::write(&manifest, b"{not json").unwrap();
        assert!(!is_complete(dir.path(), &manifest));
        let _ = fs::remove_file(manifest);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_listed_and_checked_without_following_them() {
        let dir = tree();
        std::os::unix::fs::symlink("a.txt", dir.path().join("link")).unwrap();
        std::os::unix::fs::symlink("nowhere", dir.path().join("dangling"))
            .unwrap();
        let manifest = publish(&dir);
        assert!(is_complete(dir.path(), &manifest));

        fs::remove_file(dir.path().join("link")).unwrap();
        assert!(!is_complete(dir.path(), &manifest));
        let _ = fs::remove_file(manifest);
    }
}
