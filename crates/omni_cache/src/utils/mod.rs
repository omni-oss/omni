use std::path::{Path, PathBuf};

use system_traits::FsMetadata;

pub fn project_dirname(project_name: &str) -> String {
    bs58::encode(project_name).into_string()
}

pub fn relpath<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base)
        .expect("path is not a child of base")
}

pub fn has_globs(path: &str) -> bool {
    path.contains("*")
        || path.contains("?")
        || path.contains("[")
        || path.contains("{")
}

fn remove_globs(path: &Path) -> &Path {
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

pub fn topmost_dir<'a>(
    sys: impl FsMetadata,
    paths: &'a [PathBuf],
    ws_root_dir: &'a Path,
) -> &'a Path {
    if paths.is_empty() {
        return ws_root_dir;
    }

    // Normalize all paths by removing globs and adjusting to directories
    let dirs: Vec<&Path> = paths
        .iter()
        .map(|p| get_dir(&sys, remove_globs(p.as_path())))
        .collect();

    // Start with the first path as candidate for common ancestor
    let mut common = dirs[0];

    for dir in &dirs[1..] {
        // Walk up from 'common' until 'dir' starts with it
        while !dir.starts_with(common) {
            common = common.parent().unwrap_or(ws_root_dir);
        }
    }

    common
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

        let result = topmost_dir(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, Path::new("/root/nested"));
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

        let result = topmost_dir(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, Path::new("/root/nested"));
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

        let result = topmost_dir(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, Path::new("/root/nested/project-1"));
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

        let result = topmost_dir(sys, &paths[..], &ws_root_dir);

        assert_eq!(result, Path::new("/root"));
    }
}
