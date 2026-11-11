use std::{collections::HashMap, sync::Mutex};

/// Tracks whether each source in an expansion is local, carrying the answer
/// from parent to child by qualified id.
///
/// A source is local only if it is reached from the workspace alone: it is not
/// pinned (materialized from git) and neither is anything above it. Locality is
/// carried through expansion rather than inferred from paths, so a
/// workspace-local source that points into the store keeps workspace ignores
/// for its local children, and a child of a git source never does.
///
/// A child's qualified id is its parent's qualified id, `::`, and the child's
/// own declared id. The declared id is known, so the parent is found by
/// removing that exact suffix; the qualified id is never split, so ids may
/// contain `::`. Qualified ids are unique per path through the expansion, so two
/// sources that share a root do not share locality. Parents are classified
/// before their children.
#[derive(Debug, Default)]
pub struct Locality {
    by_qualified_id: Mutex<HashMap<String, bool>>,
}

impl Locality {
    /// Resolve and record the locality of the source at `qualified_id`, whose
    /// own authored id is `declared_id`. `pinned` is whether it was materialized
    /// from git. `forced_non_local` marks a top-level source that a git pack
    /// contributed.
    pub fn resolve(
        &self,
        qualified_id: &str,
        declared_id: &str,
        depth: usize,
        pinned: bool,
        forced_non_local: bool,
    ) -> bool {
        let mut known = self
            .by_qualified_id
            .lock()
            .expect("locality mutex poisoned");
        let parent_local = depth == 0
            || qualified_id
                .strip_suffix(declared_id)
                .and_then(|rest| rest.strip_suffix("::"))
                .and_then(|parent| known.get(parent).copied())
                .unwrap_or(false);
        let is_local = !pinned && !forced_non_local && parent_local;
        known.insert(qualified_id.to_string(), is_local);
        is_local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `resolve` for a source whose qualified id is `qid` and whose own id is
    /// the last `::` segment.
    fn at(l: &Locality, qid: &str, depth: usize, pinned: bool) -> bool {
        let own = qid.rsplit("::").next().unwrap();
        l.resolve(qid, own, depth, pinned, false)
    }

    #[test]
    fn a_workspace_source_and_its_local_children_are_local() {
        let l = Locality::default();
        assert!(at(&l, "kit", 0, false));
        assert!(at(&l, "kit::inner", 1, false));
        assert!(at(&l, "kit::inner::leaf", 2, false));
    }

    #[test]
    fn a_pinned_source_and_everything_below_it_is_not_local() {
        let l = Locality::default();
        assert!(!at(&l, "kit", 0, true));
        assert!(!at(&l, "kit::inner", 1, false));
        assert!(!at(&l, "kit::inner::leaf", 2, false));
    }

    #[test]
    fn a_git_child_of_a_local_source_is_not_local_and_taints_its_children() {
        let l = Locality::default();
        assert!(at(&l, "kit", 0, false));
        assert!(!at(&l, "kit::remote", 1, true));
        assert!(!at(&l, "kit::remote::leaf", 2, false));
        assert!(at(&l, "kit::sibling", 1, false));
    }

    #[test]
    fn a_forced_non_local_top_level_source_taints_its_children() {
        let l = Locality::default();
        assert!(!l.resolve("kit/skills", "kit/skills", 0, false, true));
        assert!(!at(&l, "kit/skills::member", 1, false));
    }

    #[test]
    fn an_unrecorded_parent_is_treated_as_not_local() {
        let l = Locality::default();
        assert!(!at(&l, "ghost::child", 1, false));
    }

    /// An id may contain the separator. `kit`'s child is `inner::leaf`, so the
    /// parent of `kit::inner::leaf` is `kit`, not `kit::inner`.
    #[test]
    fn an_id_containing_the_separator_resolves_against_the_real_parent() {
        let l = Locality::default();
        assert!(at(&l, "kit", 0, false));
        assert!(!at(&l, "inner", 0, true));
        assert!(l.resolve("kit::inner::leaf", "inner::leaf", 1, false, false));
    }

    /// Two sources that happen to share a root are told apart by their qualified
    /// ids, so a local entry is not tainted by a git entry for the same root.
    #[test]
    fn sources_with_the_same_root_do_not_share_locality() {
        let l = Locality::default();
        assert!(!at(&l, "remote", 0, true));
        assert!(at(&l, "local", 0, false));
        assert!(at(&l, "local::child", 1, false));
        assert!(!at(&l, "remote::child", 1, false));
    }
}
