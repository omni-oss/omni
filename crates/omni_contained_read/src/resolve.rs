use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Resolve `path` to the real location an operation would reach, following
/// symlinks in every component.
///
/// This is a copy of the broker's private `resolve_real_path`
/// (`omni_capability_sys::sys`), kept in step with it by a parity test, so the
/// alias check judges a path exactly as the broker authorizes it. Fully
/// existing paths are canonicalized; for a path that does not exist, the
/// deepest existing component is resolved (a dangling final symlink resolves
/// to its link target) and the absent tail is re-appended. `None` means the
/// path could not be resolved at all and the lexical path stands.
///
/// Precondition: `path` is root-anchored (checked in debug builds).
pub fn resolve_real_path(path: &Path) -> Option<PathBuf> {
    debug_assert!(
        path.has_root(),
        "alias resolution expects a root-anchored path, got {path:?}"
    );
    if let Ok(real) = std::fs::canonicalize(path) {
        return Some(real);
    }

    let mut tail: Vec<OsString> = Vec::new();
    let mut current = path;
    loop {
        if std::fs::symlink_metadata(current).is_ok() {
            let base = match std::fs::canonicalize(current) {
                Ok(real) => real,
                Err(_) => {
                    let link_target = std::fs::read_link(current).ok()?;
                    let parent = current.parent()?;
                    let real_parent = std::fs::canonicalize(parent).ok()?;
                    real_parent.join(link_target)
                }
            };
            let mut resolved = base;
            for name in tail.iter().rev() {
                resolved.push(name.as_os_str());
            }
            return Some(resolved);
        }
        let parent = current.parent()?;
        let name = current.file_name()?;
        tail.push(name.to_owned());
        if skip_as_root(parent) {
            return None;
        }
        current = parent;
    }
}

#[cfg(windows)]
#[inline(always)]
fn skip_as_root(path: &Path) -> bool {
    path.parent().is_none()
}

#[cfg(not(windows))]
#[inline(always)]
fn skip_as_root(_path: &Path) -> bool {
    false
}
