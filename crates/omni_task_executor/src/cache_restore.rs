//! Restore through retained directory capabilities, never checked-then-reopened
//! absolute destination paths. The workspace and local cache are trusted roots;
//! components below the workspace may be concurrently swapped.

use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::DirExt as _;
use cap_std::fs::{Dir, OpenOptions};

use omni_cache::CachedTaskExecution;

struct Output {
    source: PathBuf,
    relative: PathBuf,
}

/// Prepare every destination before writing any output. A rejected entry must
/// cause execution, not a cache hit with some promised outputs silently absent.
pub(crate) struct Restore {
    root: Dir,
    outputs: Vec<Output>,
}

impl Restore {
    pub(crate) fn prepare(
        workspace: &Path,
        entry: &CachedTaskExecution,
    ) -> io::Result<Self> {
        let root =
            Dir::open_ambient_dir(workspace, cap_std::ambient_authority())?;
        let mut outputs = Vec::with_capacity(entry.files.len());
        for file in &entry.files {
            let path = file.original_path.path().map_err(|_| unsafe_path())?;
            // Validate lexical confinement, but retain the original components:
            // `link/../out` must not hide a symlink by normalizing it away.
            relative_destination(workspace, path)?;
            let relative = path
                .strip_prefix(workspace)
                .map_err(|_| unsafe_path())?
                .to_path_buf();
            let (parents, name) = destination(&root, &relative)?;
            let parent = parents.last().expect("root is retained");
            match parent.symlink_metadata(&name) {
                Ok(meta) if meta.is_file() => continue, // Existing outputs are retained.
                Ok(_) => return Err(unsafe_path()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            if !std::fs::symlink_metadata(&file.cached_path)?.is_file() {
                return Err(unsafe_path());
            }
            outputs.push(Output {
                source: file.cached_path.clone(),
                relative,
            });
        }
        Ok(Self { root, outputs })
    }

    pub(crate) fn write(self) -> io::Result<()> {
        for output in self.outputs {
            // Reopen safely, not by pathname. Keeping only this file's chain
            // bounds descriptors by path depth, not by archive entry count.
            let (parents, name) = destination(&self.root, &output.relative)?;
            write_output(&output.source, &parents, &name)?;
        }
        Ok(())
    }
}

fn relative_destination(workspace: &Path, path: &Path) -> io::Result<PathBuf> {
    let path = path.strip_prefix(workspace).map_err(|_| unsafe_path())?;
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            Component::ParentDir if relative.pop() => {}
            _ => return Err(unsafe_path()),
        }
    }
    if !omni_types::is_confined_relative(&relative) {
        return Err(unsafe_path());
    }
    Ok(relative)
}

fn destination(
    root: &Dir,
    relative: &Path,
) -> io::Result<(Vec<Dir>, std::ffi::OsString)> {
    let mut parents = vec![root.try_clone()?];
    // The real (symlink-free) location of `parents.last()`, relative to root.
    let mut real = PathBuf::new();
    for component in relative.parent().ok_or_else(unsafe_path)?.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir if parents.len() > 1 => {
                parents.pop();
                real.pop();
                continue;
            }
            Component::Normal(name)
                if omni_types::is_confined_relative(Path::new(name)) =>
            {
                let parent = parents.last().expect("root is retained");
                match parent.symlink_metadata(name) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        // A link that stays inside the workspace is followed to
                        // its real directory, resolved by cap-std so it can
                        // never leave `root`; an escaping link is an error.
                        // Every component is then re-opened without following
                        // links, so a later swap fails closed.
                        let target = root.canonicalize(real.join(name))?;
                        parents = vec![root.try_clone()?];
                        for part in target.components() {
                            let Component::Normal(part) = part else {
                                return Err(unsafe_path());
                            };
                            let next = parents
                                .last()
                                .expect("root is retained")
                                .open_dir_nofollow(part)?;
                            parents.push(next);
                        }
                        real = target;
                    }
                    Ok(_) => {
                        parents.push(parent.open_dir_nofollow(name)?);
                        real.push(name);
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        parent.create_dir(name)?;
                        parents.push(parent.open_dir_nofollow(name)?);
                        real.push(name);
                    }
                    Err(e) => return Err(e),
                }
            }
            _ => return Err(unsafe_path()),
        }
    }
    let name = relative.file_name().ok_or_else(unsafe_path)?;
    if !omni_types::is_confined_relative(Path::new(name)) {
        return Err(unsafe_path());
    }
    Ok((parents, name.into()))
}

fn write_output(
    source: &Path,
    parents: &[Dir],
    name: &std::ffi::OsStr,
) -> io::Result<()> {
    let parent = parents.last().expect("root is retained");
    let mut source = std::fs::File::open(source)?;
    if !source.metadata()?.is_file() {
        return Err(unsafe_path());
    }
    // Never follow a final symlink or write an existing hardlink.
    let mut dest = parent
        .open_with(name, OpenOptions::new().write(true).create_new(true))?;
    let result = (|| {
        io::copy(&mut source, &mut dest)?;
        dest.set_permissions(cap_std::fs::Permissions::from_std(
            source.metadata()?.permissions(),
        ))
    })();
    drop(dest);
    if result.is_err() {
        // Unlink via the retained parent, never through a swapped full path.
        let _ = parent.remove_file(name);
    }
    result
}

fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unsafe cache restore destination",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &Path, destination: &Path) -> CachedTaskExecution {
        CachedTaskExecution {
            project_name: "p".into(),
            task_name: "task".into(),
            task_exec: None,
            task_retry_exec: None,
            digest: [0; 32],
            dependency_digests: vec![],
            logs_path: None,
            files: vec![omni_cache::CachedFileOutput {
                cached_path: source.into(),
                original_path: omni_types::OmniPath::new(destination),
            }],
            exit_code: 0,
            execution_duration: std::time::Duration::ZERO,
            execution_time: time::OffsetDateTime::UNIX_EPOCH,
            tries: 1,
        }
    }

    fn test_destination(
        root: &Path,
        relative: &str,
    ) -> io::Result<(Vec<Dir>, std::ffi::OsString)> {
        let root = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
        destination(&root, Path::new(relative))
    }

    #[test]
    fn regular_files_are_created_inside() {
        let root = tempfile::tempdir().unwrap();
        let (parents, name) = test_destination(root.path(), "a/b/out").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"ok").unwrap();
        write_output(source.path(), &parents, &name).unwrap();
        assert_eq!(std::fs::read(root.path().join("a/b/out")).unwrap(), b"ok");
    }

    #[test]
    fn preparation_and_write_restore_missing_outputs_without_mutating_cache() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"cached").unwrap();
        let path = root.path().join("out/file");
        Restore::prepare(root.path(), &entry(source.path(), &path))
            .unwrap()
            .write()
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"cached");
        std::fs::write(path, b"modified").unwrap();
        assert_eq!(std::fs::read(source.path()).unwrap(), b"cached");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn final_symlink_swap_causes_restore_failure_without_outside_write() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), b"keep").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"evil").unwrap();
        let path = root.path().join("out");
        let restore =
            Restore::prepare(root.path(), &entry(source.path(), &path))
                .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(outside.path(), &path).unwrap();
        assert!(
            restore.write().is_err(),
            "failure must reach the executor's miss fallback"
        );
        assert_eq!(std::fs::read(outside.path()).unwrap(), b"keep");
    }

    #[test]
    fn sibling_project_outputs_are_normalized_but_workspace_escapes_are_refused()
     {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            relative_destination(
                root.path(),
                &root.path().join("p/../shared/out")
            )
            .unwrap(),
            Path::new("shared/out"),
        );
        assert!(
            relative_destination(root.path(), &root.path().join("../out"))
                .is_err()
        );
        assert!(
            relative_destination(root.path(), &root.path().join("p/../../out"))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn swapping_parent_after_preparation_cannot_redirect_write() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (parents, name) = test_destination(root.path(), "a/out").unwrap();
        std::fs::rename(root.path().join("a"), root.path().join("old"))
            .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("a"))
            .unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        write_output(source.path(), &parents, &name).unwrap();
        assert!(!outside.path().join("out").exists());
        assert!(root.path().join("old/out").exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn preexisting_parent_symlink_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), root.path().join("a"))
            .unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(
            outside.path(),
            root.path().join("a"),
        )
        .unwrap();
        assert!(test_destination(root.path(), "a/out").is_err());
        assert!(
            test_destination(root.path(), "a/../out").is_err(),
            "normalization must not hide an existing symlink"
        );
        assert!(!outside.path().join("out").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_staying_inside_the_workspace_is_followed_to_its_real_directory()
     {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("real")).unwrap();
        std::os::unix::fs::symlink("real", root.path().join("alias")).unwrap();
        let (parents, name) =
            test_destination(root.path(), "alias/out").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        write_output(source.path(), &parents, &name).unwrap();
        assert!(root.path().join("real/out").exists());
    }

    #[test]
    fn final_hardlink_appearing_after_preparation_is_not_written() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("victim"), b"keep").unwrap();
        let (parents, name) = test_destination(root.path(), "out").unwrap();
        std::fs::hard_link(
            outside.path().join("victim"),
            root.path().join("out"),
        )
        .unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        assert!(write_output(source.path(), &parents, &name).is_err());
        assert_eq!(
            std::fs::read(outside.path().join("victim")).unwrap(),
            b"keep"
        );
    }

    #[cfg(windows)]
    #[test]
    fn preexisting_parent_junction_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(test_destination(root.path(), "junction/out").is_err());
        assert!(!outside.path().join("out").exists());
    }

    #[cfg(windows)]
    #[test]
    fn retained_windows_parents_cannot_be_renamed() {
        let root = tempfile::tempdir().unwrap();
        let (_parents, _name) =
            test_destination(root.path(), "a/b/out").unwrap();
        assert!(
            std::fs::rename(root.path().join("a"), root.path().join("old"))
                .is_err()
        );
        assert!(
            std::fs::rename(root.path().join("a/b"), root.path().join("other"))
                .is_err()
        );
    }
}
