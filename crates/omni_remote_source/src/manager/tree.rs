//! A published store tree: its completeness manifest and read-only seal.
//!
//! The manifest is written last, inside the pending directory, so a tree that
//! has one was fully exported. A reader that finds files missing or the wrong
//! size (a cleaner deleted part of the tree) treats the tree as absent and
//! re-exports it.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// File name of the manifest inside a published tree.
pub(super) const MANIFEST_NAME: &str = ".manifest";

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

/// Record `dir`'s files in its manifest and make them read-only. Directories
/// stay writable so a prune can delete the tree.
pub(super) fn seal(dir: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = Manifest { files };
    let body = serde_json::to_vec(&manifest).map_err(io::Error::other)?;
    fs::write(dir.join(MANIFEST_NAME), body)?;

    for entry in &manifest.files {
        if entry.symlink {
            continue;
        }
        let path = dir.join(&entry.path);
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions)?;
    }
    let manifest_path = dir.join(MANIFEST_NAME);
    let mut permissions = fs::metadata(&manifest_path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&manifest_path, permissions)
}

/// Whether `dir` is a complete published tree: it has a manifest and every
/// listed file exists with its recorded size.
pub(super) fn is_complete(dir: &Path) -> bool {
    let Ok(body) = fs::read(dir.join(MANIFEST_NAME)) else {
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
            if path == root.join(MANIFEST_NAME) {
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

    #[test]
    fn a_sealed_tree_is_complete_and_its_files_are_read_only() {
        let dir = tree();
        assert!(!is_complete(dir.path()), "no manifest yet");

        seal(dir.path()).unwrap();

        assert!(is_complete(dir.path()));
        for file in ["a.txt", "sub/b.txt", MANIFEST_NAME] {
            let meta = fs::metadata(dir.path().join(file)).unwrap();
            assert!(meta.permissions().readonly(), "{file}");
        }
        // Directories stay writable so the tree can be deleted.
        assert!(!fs::metadata(dir.path()).unwrap().permissions().readonly());
        fs::remove_dir_all(dir.path().join("sub")).unwrap();
    }

    #[test]
    fn a_missing_or_resized_file_makes_the_tree_incomplete() {
        let dir = tree();
        seal(dir.path()).unwrap();
        assert!(is_complete(dir.path()));

        // A cleaner removed one file.
        fs::remove_file(dir.path().join("sub/b.txt")).unwrap();
        assert!(!is_complete(dir.path()));

        // A file was replaced by one of another size.
        fs::write(dir.path().join("sub/b.txt"), b"much longer").unwrap();
        assert!(!is_complete(dir.path()));
    }

    #[test]
    fn a_corrupt_manifest_is_incomplete_not_an_error() {
        let dir = tree();
        seal(dir.path()).unwrap();
        let manifest = dir.path().join(MANIFEST_NAME);
        let mut permissions = fs::metadata(&manifest).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&manifest, permissions).unwrap();
        fs::write(&manifest, b"{not json").unwrap();
        assert!(!is_complete(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_listed_and_checked_without_following_them() {
        let dir = tree();
        std::os::unix::fs::symlink("a.txt", dir.path().join("link")).unwrap();
        std::os::unix::fs::symlink("nowhere", dir.path().join("dangling"))
            .unwrap();
        seal(dir.path()).unwrap();
        assert!(is_complete(dir.path()));

        fs::remove_file(dir.path().join("link")).unwrap();
        assert!(!is_complete(dir.path()));
    }
}
