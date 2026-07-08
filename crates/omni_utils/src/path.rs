use std::path::{Path, PathBuf};

pub use pathdiff::diff_paths as diff;
use system_traits::FsMetadata;

pub fn relpath<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base)
        .expect("path is not a child of base")
}

pub fn has_globs(path: &str) -> bool {
    if path.contains("*")
        || path.contains("[")
        || path.contains("{")
        || (!cfg!(windows) && path.contains("?"))
    {
        return true;
    }

    if cfg!(windows)
        && (path.starts_with("//?/") || path.starts_with("\\\\?\\"))
        && path.chars().filter(|c| *c == '?').count() > 1
    {
        return true;
    }

    return false;
}

/// Fast equivalent of [`Path::starts_with`] for **absolute, normalized**
/// paths (no `.`/`..` components, no redundant separators).
///
/// [`Path::starts_with`] drives the `std::path::Components` state machine over
/// both operands, which is comparatively expensive and shows up as a dominant
/// cost when it is called for every walked file against every project prefix
/// (see `omni_collector`). Because the collector only ever compares canonical
/// absolute paths produced by the directory walker and `std::path::absolute`,
/// a byte-level prefix comparison with a component-boundary check is
/// semantically equivalent and avoids the component iteration entirely.
///
/// On non-unix targets this falls back to [`Path::starts_with`] to sidestep
/// separator/case-folding subtleties.
pub fn starts_with_path(path: &Path, base: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        let path_bytes = path.as_os_str().as_bytes();
        let mut base_bytes = base.as_os_str().as_bytes();

        // Mirror `Path::starts_with`, which ignores a single trailing
        // separator on the base (e.g. `/foo/` is a prefix of `/foo/bar`).
        while base_bytes.len() > 1 && base_bytes[base_bytes.len() - 1] == b'/' {
            base_bytes = &base_bytes[..base_bytes.len() - 1];
        }

        if base_bytes.is_empty() {
            return true;
        }

        if path_bytes.len() < base_bytes.len()
            || path_bytes[..base_bytes.len()] != *base_bytes
        {
            return false;
        }

        // Only match on a component boundary so `/foo` is not treated as a
        // prefix of `/foobar`. The boundary holds when the paths are equal,
        // when `base` is the root (ends in a separator), or when the next
        // byte of `path` is a separator.
        path_bytes.len() == base_bytes.len()
            || base_bytes[base_bytes.len() - 1] == b'/'
            || path_bytes[base_bytes.len()] == b'/'
    }

    #[cfg(not(unix))]
    {
        path.starts_with(base)
    }
}

pub fn remove_globs(path: &Path) -> &Path {
    if !has_globs(path.to_string_lossy().as_ref()) {
        return path;
    }

    let mut current = path;

    // ignore all the glob portions of a path
    for parent in current.ancestors() {
        let str = parent.to_string_lossy();
        if !has_globs(&str) {
            return parent;
        }

        current = parent;
    }

    current
}

fn get_dir<'a>(sys: &impl FsMetadata, path: &'a Path) -> &'a Path {
    if sys.fs_is_dir_no_err(path) {
        path
    } else {
        path.parent().expect("path should have parent")
    }
}

pub fn topmost_dirs<'a>(
    sys: impl FsMetadata,
    paths: &'a [PathBuf],
    ws_root_dir: &'a Path,
) -> Vec<&'a Path> {
    if paths.is_empty() {
        return vec![ws_root_dir];
    }

    for path in paths {
        if !path.starts_with(ws_root_dir) {
            return vec![ws_root_dir];
        }
    }

    // Normalize all paths by removing globs and adjusting to directories
    let dirs: Vec<&Path> = paths
        .iter()
        .map(|p| get_dir(&sys, remove_globs(p.as_path())))
        .collect();

    let mut topmost_dirs: Vec<&Path> = Vec::new();

    'outer: for dir in dirs {
        // If dir is already contained in an existing topmost dir, skip it
        if topmost_dirs.iter().any(|&t| dir.starts_with(t)) {
            continue 'outer;
        }

        // Remove any existing topmost dir that is inside this dir
        topmost_dirs.retain(|&t| !t.starts_with(dir));

        topmost_dirs.push(dir);
    }

    topmost_dirs
}

pub fn path_safe(text: &str) -> String {
    bs58::encode(text).into_string()
}

pub fn clean(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut components = path.components();

    if cfg!(windows) {
        use std::path::{Component, Prefix};

        path_clean::clean(match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) | Prefix::Disk(drive) => {
                    let drive = (drive as char).to_ascii_uppercase();
                    let mut out = PathBuf::from(format!("{drive}:\\"));
                    out.extend(components);
                    out
                }
                Prefix::VerbatimUNC(server, share)
                | Prefix::UNC(server, share) => {
                    let mut out = PathBuf::from(format!(
                        r"\\{}\{}",
                        server.to_string_lossy(),
                        share.to_string_lossy()
                    ));
                    out.extend(components);
                    out
                }
                _ => path.to_path_buf(),
            },
            _ => path.to_path_buf(),
        })
    } else {
        path_clean::clean(path)
    }
}

#[cfg(test)]
mod tests {
    use system_traits::{FsCreateDirAll as _, impls::InMemorySys};

    use super::*;
    use std::path::Path;

    #[test]
    fn test_remove_globs() {
        let path = Path::new("/test/**/*.txt");

        let result = remove_globs(path);

        assert_eq!(result, Path::new("/test"));
    }

    #[test]
    fn test_topmost_dir_with_outside_project() {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");
        sys.fs_create_dir_all("/root/nested/project-2")
            .expect("Can't create project-2 dir");

        let paths = vec![
            PathBuf::from("/root/nested/project-1/project.omni.yaml"),
            PathBuf::from("/root/nested/project-2/project.omni.yaml"),
            PathBuf::from("/root/nested/**.*"),
        ];

        let ws_root_dir = Path::new("/root");

        let result = topmost_dirs(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, &[Path::new("/root/nested")]);
    }

    #[test]
    fn test_topmost_dir_with_same_level() {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");
        sys.fs_create_dir_all("/root/nested/project-2")
            .expect("Can't create project-2 dir");

        let paths = vec![
            PathBuf::from("/root/nested/project-1/project.omni.yaml"),
            PathBuf::from("/root/nested/project-2/project.omni.yaml"),
            PathBuf::from("/root/nested/project-3/project.omni.yaml"),
        ];

        let ws_root_dir = Path::new("/root");

        let result = topmost_dirs(sys, &paths[..], &ws_root_dir);

        assert_eq!(
            result,
            &[
                Path::new("/root/nested/project-1"),
                Path::new("/root/nested/project-2"),
                Path::new("/root/nested/project-3"),
            ]
        );
    }

    #[test]
    fn test_topmost_dir_with_projects_at_different_levels() {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");
        sys.fs_create_dir_all("/root/nested/nested2/project-2")
            .expect("Can't create project-2 dir");

        let paths = vec![
            PathBuf::from("/root/nested/project-1/src/a.txt"),
            PathBuf::from("/root/nested/project-1/project.omni.yaml"),
            PathBuf::from("/root/nested/nested2/project-2/test.txt"),
        ];

        let ws_root_dir = Path::new("/root");

        let result = topmost_dirs(sys, &paths[..], &ws_root_dir);

        assert_eq!(
            result,
            &[
                Path::new("/root/nested/project-1"),
                Path::new("/root/nested/nested2/project-2"),
            ]
        );
    }

    #[test]
    fn test_topmost_dir_with_inside_project_different_levels() {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");
        sys.fs_create_dir_all("/root/nested/project-2")
            .expect("Can't create project-2 dir");

        let paths = vec![
            PathBuf::from("/root/nested/project-1/src/a.txt"),
            PathBuf::from("/root/nested/project-1/project.omni.yaml"),
            PathBuf::from("/root/nested/project-1/src/nested/a.txt"),
        ];

        let ws_root_dir = Path::new("/root");

        let result = topmost_dirs(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, &[Path::new("/root/nested/project-1")]);
    }

    #[test]
    fn test_topmost_dir_should_ignore_glob_components() {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");
        sys.fs_create_dir_all("/root/nested/project-2")
            .expect("Can't create project-2 dir");

        let paths = vec![
            PathBuf::from("/root/nested/project-1/src/a.txt"),
            PathBuf::from("/root/nested/project-1/project.omni.yaml"),
            PathBuf::from("/root/nested/project-1/src/nested/a.txt"),
            PathBuf::from("/root/nested/project-1/src/**/*.txt"),
            PathBuf::from("/root/**.*.txt"),
        ];

        let ws_root_dir = Path::new("/root");

        let result = topmost_dirs(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, &[Path::new("/root")]);
    }

    #[cfg(unix)]
    #[test]
    fn test_starts_with_path_matches_std() {
        let cases = [
            ("/foo/bar", "/foo", true),
            ("/foo/bar", "/foo/", true),
            ("/foo/bar", "/foo/bar", true),
            ("/foo/bar", "/foo/ba", false),
            ("/foobar", "/foo", false),
            ("/foo/bar/baz", "/foo/bar", true),
            ("/foo", "/foo/bar", false),
            ("/foo/bar", "/", true),
            ("/foo/bar", "/other", false),
            ("/foo/bar", "", true),
        ];

        for (path, base, expected) in cases {
            let path = Path::new(path);
            let base = Path::new(base);
            assert_eq!(
                starts_with_path(path, base),
                expected,
                "starts_with_path({path:?}, {base:?})"
            );
            // Parity with the std implementation it replaces.
            assert_eq!(
                starts_with_path(path, base),
                path.starts_with(base),
                "std parity for ({path:?}, {base:?})"
            );
        }
    }

    #[test]
    fn test_clean() {
        let path = Path::new("/foo/./bar/../baz");
        let cleaned = clean(path);
        assert_eq!(cleaned, Path::new("/foo/baz"));
    }

    #[cfg(windows)]
    #[test]
    fn test_clean_windows() {
        let path = Path::new("\\\\?\\C:\\foo\\.\\bar\\..\\baz");
        let cleaned = clean(path);
        assert_eq!(cleaned, Path::new("C:\\foo\\baz"));
    }
}
