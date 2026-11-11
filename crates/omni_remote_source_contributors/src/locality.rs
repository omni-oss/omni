use std::{collections::HashMap, sync::Mutex};

/// Tracks whether each source in an expansion is local, carrying the answer
/// from parent to child by qualified id.
///
/// A source is local only if it is reached from the workspace alone: it is not
/// pinned (materialized from git) and neither is anything above it. Locality is
/// carried through expansion rather than inferred from paths, so a
/// workspace-local source that points into the store keeps workspace ignores
/// for its local children, and a child of a git source never does. Parents are
/// classified before their children.
#[derive(Debug, Default)]
pub struct Locality {
    by_id: Mutex<HashMap<String, bool>>,
}

impl Locality {
    /// Resolve and record the locality of `qualified_id`. `pinned` is whether
    /// the source was materialized from git. `forced_non_local` marks a
    /// top-level source that a git pack contributed.
    pub fn resolve(
        &self,
        qualified_id: &str,
        depth: usize,
        pinned: bool,
        forced_non_local: bool,
    ) -> bool {
        let mut by_id = self.by_id.lock().expect("locality mutex poisoned");
        let parent_local = depth == 0
            || qualified_id
                .rsplit_once("::")
                .and_then(|(parent, _)| by_id.get(parent).copied())
                .unwrap_or(false);
        let is_local = !pinned && !forced_non_local && parent_local;
        by_id.insert(qualified_id.to_string(), is_local);
        is_local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_source_and_its_local_children_are_local() {
        let l = Locality::default();
        assert!(l.resolve("kit", 0, false, false));
        assert!(l.resolve("kit::inner", 1, false, false));
        assert!(l.resolve("kit::inner::leaf", 2, false, false));
    }

    #[test]
    fn a_pinned_source_and_everything_below_it_is_not_local() {
        let l = Locality::default();
        assert!(!l.resolve("kit", 0, true, false));
        assert!(!l.resolve("kit::inner", 1, false, false));
        assert!(!l.resolve("kit::inner::leaf", 2, false, false));
    }

    #[test]
    fn a_git_child_of_a_local_source_is_not_local_and_taints_its_children() {
        let l = Locality::default();
        assert!(l.resolve("kit", 0, false, false));
        assert!(!l.resolve("kit::remote", 1, true, false));
        assert!(!l.resolve("kit::remote::leaf", 2, false, false));
        assert!(l.resolve("kit::sibling", 1, false, false));
    }

    #[test]
    fn a_forced_non_local_top_level_source_taints_its_children() {
        let l = Locality::default();
        assert!(!l.resolve("kit/skills", 0, false, true));
        assert!(!l.resolve("kit/skills::member", 1, false, false));
    }

    #[test]
    fn an_unrecorded_parent_is_treated_as_not_local() {
        let l = Locality::default();
        assert!(!l.resolve("ghost::child", 1, false, false));
    }
}
