//! Restore through retained directory capabilities, never checked-then-reopened
//! absolute destination paths. The workspace and local cache are trusted roots;
//! components below the workspace may be concurrently swapped.

use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::DirExt as _;
use cap_std::fs::{Dir, OpenOptions};

use omni_cache::CachedTaskExecution;

/// Where an absolute link target must start to count as inside the workspace.
struct Workspace {
    given: PathBuf,
    real: PathBuf,
}

impl Workspace {
    fn new(workspace: &Path) -> io::Result<Self> {
        Ok(Self {
            given: workspace.to_path_buf(),
            real: std::fs::canonicalize(workspace)?,
        })
    }

    /// A link's contents as a path relative to the workspace root, resolved
    /// from `real` (the link's directory) when it is relative itself.
    fn relative_target(
        &self,
        contents: &Path,
        real: &Path,
    ) -> io::Result<PathBuf> {
        if contents.is_absolute() {
            contents
                .strip_prefix(&self.real)
                .or_else(|_| contents.strip_prefix(&self.given))
                .map(Path::to_path_buf)
                .map_err(|_| unsafe_path())
        } else {
            Ok(real.join(contents))
        }
    }
}

struct Output {
    source: PathBuf,
    relative: PathBuf,
}

/// Prepare every destination before writing any output. A rejected entry must
/// cause execution, not a cache hit with some promised outputs silently absent.
pub(crate) struct Restore {
    root: Dir,
    workspace: Workspace,
    outputs: Vec<Output>,
}

impl Restore {
    pub(crate) fn prepare(
        workspace: &Path,
        entry: &CachedTaskExecution,
    ) -> io::Result<Self> {
        let root =
            Dir::open_ambient_dir(workspace, cap_std::ambient_authority())?;
        let anchors = Workspace::new(workspace)?;
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
            if !marker_allows_destination(entry.path_format, &relative) {
                return Err(unsafe_path());
            }
            let (parents, name) = destination(&root, &anchors, &relative)?;
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
        Ok(Self {
            root,
            workspace: anchors,
            outputs,
        })
    }

    pub(crate) fn write(self) -> io::Result<()> {
        for output in self.outputs {
            // Reopen safely, not by pathname. Keeping only this file's chain
            // bounds descriptors by path depth, not by archive entry count.
            let (parents, name) =
                destination(&self.root, &self.workspace, &output.relative)?;
            write_output(&output.source, &parents, &name)?;
        }
        Ok(())
    }
}

/// Whether the entry's path-format marker allows this *resolved* destination.
///
/// On Windows the destination has already been resolved to native `\`
/// separators, so a backslash is always a separator here and says nothing
/// about the stored form. That form is checked before resolution (see
/// `CachedTaskExecution::paths_match_host_format`); only the marker itself
/// can be validated at this point. Elsewhere a backslash in the resolved
/// path is a literal name, which a legacy (unmarked) entry cannot vouch for.
fn marker_allows_destination(format: u8, relative: &Path) -> bool {
    marker_allows_destination_on(format, relative, cfg!(windows))
}

fn marker_allows_destination_on(
    format: u8,
    relative: &Path,
    windows: bool,
) -> bool {
    if windows {
        omni_cache::path_format_allows_on(format, Path::new(""), true)
    } else {
        omni_cache::path_format_allows_on(format, relative, false)
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
    workspace: &Workspace,
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
                let opened = match parent.symlink_metadata(name) {
                    Ok(meta) if !meta.file_type().is_symlink() => {
                        parent.open_dir_nofollow(name).map(Some)
                    }
                    Ok(_) => Ok(None),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        parent.create_dir(name)?;
                        parent.open_dir_nofollow(name).map(Some)
                    }
                    Err(e) => Err(e),
                };
                match opened {
                    Ok(Some(dir)) => {
                        parents.push(dir);
                        real.push(name);
                    }
                    // A link (or a junction cap-std does not classify as one):
                    // follow it only if it stays inside the workspace, then
                    // re-open every real component without following links.
                    Ok(None) | Err(_) if is_link(workspace, &real, name) => {
                        let target =
                            resolve_link(root, workspace, parent, &real, name)?;
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
                    Ok(None) => return Err(unsafe_path()),
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

/// True when `real/name` under the workspace is a symlink or a Windows junction
/// (std reports both as symlinks).
fn is_link(workspace: &Workspace, real: &Path, name: &std::ffi::OsStr) -> bool {
    std::fs::symlink_metadata(workspace.real.join(real).join(name))
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// The workspace-relative real directory a link leads to, or an error when it
/// leads outside. cap-std resolves relative and absolute links it can; the std
/// fallback covers junctions and platform spellings, and its result is only
/// ever used after it is proven to be under the real workspace root.
fn resolve_link(
    root: &Dir,
    workspace: &Workspace,
    parent: &Dir,
    real: &Path,
    name: &std::ffi::OsStr,
) -> io::Result<PathBuf> {
    let by_cap = parent.read_link_contents(name).and_then(|contents| {
        root.canonicalize(workspace.relative_target(&contents, real)?)
    });
    if let Ok(target) = by_cap {
        return Ok(target);
    }
    let resolved = std::fs::canonicalize(workspace.real.join(real).join(name))?;
    resolved
        .strip_prefix(&workspace.real)
        .map(Path::to_path_buf)
        .map_err(|_| unsafe_path())
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

    #[test]
    fn marked_entry_restores_a_resolved_native_windows_destination() {
        use omni_cache::{PATH_FORMAT_LEGACY, PATH_FORMAT_PORTABLE};
        // On Windows the destination was resolved to native separators before
        // the marker is consulted, so nested outputs of a *marked* entry must
        // still be allowed; only the marker itself is validated there.
        let resolved = Path::new("packages\\a\\dist\\a.js");
        assert!(super::marker_allows_destination_on(
            PATH_FORMAT_PORTABLE,
            resolved,
            true
        ));
        assert!(super::marker_allows_destination_on(
            PATH_FORMAT_LEGACY,
            resolved,
            true
        ));
        assert!(!super::marker_allows_destination_on(9, resolved, true));
    }

    #[test]
    fn off_windows_only_a_marked_entry_may_name_a_backslash_file() {
        use omni_cache::{PATH_FORMAT_LEGACY, PATH_FORMAT_PORTABLE};
        let literal = Path::new("dist\\a.js");
        assert!(super::marker_allows_destination_on(
            PATH_FORMAT_PORTABLE,
            literal,
            false
        ));
        assert!(!super::marker_allows_destination_on(
            PATH_FORMAT_LEGACY,
            literal,
            false
        ));
        assert!(!super::marker_allows_destination_on(9, literal, false));
    }
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
            path_format: omni_cache::PATH_FORMAT_LEGACY,
        }
    }

    fn test_destination(
        root_path: &Path,
        relative: &str,
    ) -> io::Result<(Vec<Dir>, std::ffi::OsString)> {
        let root =
            Dir::open_ambient_dir(root_path, cap_std::ambient_authority())?;
        let workspace = Workspace::new(root_path)?;
        destination(&root, &workspace, Path::new(relative))
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

    /// Issue #62: a legacy entry that names `dist\\a.js` must not create a
    /// file with a literal backslash in its name.
    #[cfg(not(windows))]
    #[test]
    fn legacy_backslash_destination_is_rejected_not_restored_literally() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"cached").unwrap();
        let path = root.path().join("dist\\a.js");
        let prepared =
            Restore::prepare(root.path(), &entry(source.path(), &path))
                .and_then(Restore::write);
        assert!(prepared.is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn marked_nested_unicode_outputs_restore_and_existing_files_are_kept() {
        for format in [
            omni_cache::PATH_FORMAT_PORTABLE,
            omni_cache::PATH_FORMAT_LEGACY,
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(source.path(), b"cached").unwrap();
            let fresh = root.path().join("app/dist/sub/é.js");
            let existing = root.path().join("shared/ü.js");
            std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
            std::fs::write(&existing, b"local").unwrap();
            let mut e = entry(source.path(), &fresh);
            e.path_format = format;
            let mut second = e.files[0].clone();
            second.original_path = omni_types::OmniPath::new(&existing);
            e.files.push(second);
            Restore::prepare(root.path(), &e).unwrap().write().unwrap();
            assert_eq!(std::fs::read(&fresh).unwrap(), b"cached");
            assert_eq!(std::fs::read(&existing).unwrap(), b"local");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn marked_backslash_destination_is_a_literal_name_off_windows() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"cached").unwrap();
        let path = root.path().join("dist\\a.js");
        let mut e = entry(source.path(), &path);
        e.path_format = omni_cache::PATH_FORMAT_PORTABLE;
        Restore::prepare(root.path(), &e).unwrap().write().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"cached");
        assert!(!root.path().join("dist").exists());
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

    #[cfg(unix)]
    #[test]
    fn an_absolute_symlink_into_the_workspace_is_followed_but_one_outside_is_not()
     {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("real")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("real"),
            root.path().join("inside"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside"))
            .unwrap();
        let (parents, name) =
            test_destination(root.path(), "inside/out").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        write_output(source.path(), &parents, &name).unwrap();
        assert!(root.path().join("real/out").exists());
        assert!(test_destination(root.path(), "outside/out").is_err());
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
    fn a_junction_inside_the_workspace_is_followed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("real")).unwrap();
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(root.path().join("alias"))
            .arg(root.path().join("real"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let (parents, name) =
            test_destination(root.path(), "alias/out").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        write_output(source.path(), &parents, &name).unwrap();
        assert!(root.path().join("real/out").exists());
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
