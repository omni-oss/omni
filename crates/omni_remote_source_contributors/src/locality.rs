use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Tracks whether each source in an expansion is local, carrying the answer
/// from parent to child.
///
/// A source is local only if it is reached from the workspace alone: it is not
/// pinned (materialized from git) and neither is anything above it. Locality is
/// carried through expansion rather than inferred from paths, so a
/// workspace-local source that points into the store keeps workspace ignores
/// for its local children, and a child of a git source never does. A child finds
/// its parent by the parent's materialized root, which the expansion passes down
/// unchanged as the child's `parent_root`; ids are never parsed, so an id may
/// contain any characters. Parents are classified before their children.
#[derive(Debug, Default)]
pub struct Locality {
    by_root: Mutex<HashMap<PathBuf, bool>>,
}

impl Locality {
    /// Resolve and record the locality of the source materialized at `root`.
    /// `parent_root` is the root its parent was materialized at (ignored at
    /// `depth` 0, where the parent is the workspace). `pinned` is whether the
    /// source was materialized from git. `forced_non_local` marks a top-level
    /// source that a git pack contributed.
    pub fn resolve(
        &self,
        root: &Path,
        parent_root: &Path,
        depth: usize,
        pinned: bool,
        forced_non_local: bool,
    ) -> bool {
        let mut by_root = self.by_root.lock().expect("locality mutex poisoned");
        let parent_local =
            depth == 0 || by_root.get(parent_root).copied().unwrap_or(false);
        let is_local = !pinned && !forced_non_local && parent_local;
        by_root
            .entry(root.to_path_buf())
            .and_modify(|known| *known &= is_local)
            .or_insert(is_local);
        is_local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    /// `resolve(root, parent_root, depth, pinned, forced_non_local)`.
    fn resolve(
        l: &Locality,
        root: &str,
        parent_root: &str,
        depth: usize,
        pinned: bool,
        forced: bool,
    ) -> bool {
        l.resolve(&p(root), &p(parent_root), depth, pinned, forced)
    }

    #[test]
    fn a_workspace_source_and_its_local_children_are_local() {
        let l = Locality::default();
        assert!(resolve(&l, "/ws/kit", "/ws", 0, false, false));
        assert!(resolve(&l, "/ws/kit/inner", "/ws/kit", 1, false, false));
        assert!(resolve(
            &l,
            "/ws/kit/inner/leaf",
            "/ws/kit/inner",
            2,
            false,
            false
        ));
    }

    #[test]
    fn a_pinned_source_and_everything_below_it_is_not_local() {
        let l = Locality::default();
        assert!(!resolve(&l, "/store/kit", "/ws", 0, true, false));
        assert!(!resolve(
            &l,
            "/store/kit/inner",
            "/store/kit",
            1,
            false,
            false
        ));
        assert!(!resolve(
            &l,
            "/store/kit/inner/leaf",
            "/store/kit/inner",
            2,
            false,
            false
        ));
    }

    #[test]
    fn a_git_child_of_a_local_source_is_not_local_and_taints_its_children() {
        let l = Locality::default();
        assert!(resolve(&l, "/ws/kit", "/ws", 0, false, false));
        assert!(!resolve(&l, "/store/remote", "/ws/kit", 1, true, false));
        assert!(!resolve(
            &l,
            "/store/remote/leaf",
            "/store/remote",
            2,
            false,
            false
        ));
        assert!(resolve(&l, "/ws/kit/sibling", "/ws/kit", 1, false, false));
    }

    #[test]
    fn a_forced_non_local_top_level_source_taints_its_children() {
        let l = Locality::default();
        assert!(!resolve(&l, "/store/kit/skills", "/ws", 0, false, true));
        assert!(!resolve(
            &l,
            "/store/kit/skills/member",
            "/store/kit/skills",
            1,
            false,
            false
        ));
    }

    #[test]
    fn an_unrecorded_parent_is_treated_as_not_local() {
        let l = Locality::default();
        assert!(!resolve(
            &l,
            "/ws/ghost/child",
            "/ws/ghost",
            1,
            false,
            false
        ));
    }

    /// Ids are never parsed, so one containing the qualified-id separator cannot
    /// make a child resolve against the wrong parent.
    #[test]
    fn locality_does_not_depend_on_id_syntax() {
        let l = Locality::default();
        assert!(resolve(&l, "/ws/kit", "/ws", 0, false, false));
        // A sibling pack whose id would look like a parent of `inner::leaf`.
        assert!(!resolve(&l, "/store/inner", "/ws", 0, true, false));
        assert!(resolve(&l, "/ws/kit/leaf", "/ws/kit", 1, false, false));
    }

    #[test]
    fn a_shared_root_is_local_only_if_every_way_in_is_local() {
        let l = Locality::default();
        assert!(resolve(&l, "/shared", "/ws", 0, false, false));
        assert!(!resolve(&l, "/shared", "/ws", 0, true, false));
        assert!(!resolve(&l, "/shared/child", "/shared", 1, false, false));
    }
}
