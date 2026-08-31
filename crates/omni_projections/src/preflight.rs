use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use omni_projection_configurations::ExistingPolicy;

use crate::apply::{ApplierSys, PriorLink, is_foreign_existing, symlink_meta};
use crate::engine::PlannedItem;
use crate::error::{DuplicateDest, ExistingConflict, NestedDest, Result};
use crate::ledger::Ledger;

/// Detect run-wide collisions across the union of every source's planned links.
/// A collision is a self-contradictory plan and is always fatal, independent of
/// any `on_existing` policy. Two kinds are reported:
///
/// - a destination claimed by more than one distinct link (different source or
///   link kind), after collapsing exact-duplicate links; and
/// - a destination that lies strictly inside another planned directory-link
///   destination, which would write through the outer link back into a source.
///
/// Exact-duplicate links (same source, destination, and link kind) collapse to
/// one and are never reported.
pub fn collision_conflicts(
    items: &[PlannedItem],
) -> (Vec<DuplicateDest>, Vec<NestedDest>) {
    let mut by_dest: BTreeMap<&Path, Vec<&PlannedItem>> = BTreeMap::new();
    for item in items {
        by_dest
            .entry(item.pair.dest_abs.as_path())
            .or_default()
            .push(item);
    }

    let mut duplicates = Vec::new();
    for (dest, claimants) in &by_dest {
        let mut distinct: Vec<(&Path, _)> = Vec::new();
        for item in claimants {
            let id = (item.pair.source_abs.as_path(), item.link);
            if !distinct.contains(&id) {
                distinct.push(id);
            }
        }
        if distinct.len() > 1 {
            let mut sources: Vec<PathBuf> = Vec::new();
            for (source, _) in &distinct {
                let source = source.to_path_buf();
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            duplicates.push(DuplicateDest {
                dest: dest.to_path_buf(),
                sources,
            });
        }
    }

    let mut dir_dests: Vec<&Path> = items
        .iter()
        .filter(|i| i.is_dir_link)
        .map(|i| i.pair.dest_abs.as_path())
        .collect();
    dir_dests.sort();
    dir_dests.dedup();

    let mut nested = Vec::new();
    let mut seen: BTreeSet<(&Path, &Path)> = BTreeSet::new();
    for dest in by_dest.keys() {
        for dir in &dir_dests {
            if dest != dir && dest.starts_with(dir) && seen.insert((dest, dir))
            {
                nested.push(NestedDest {
                    inner: dest.to_path_buf(),
                    outer: dir.to_path_buf(),
                });
            }
        }
    }

    (duplicates, nested)
}

/// Detect existing-file conflicts across the union of planned links. For every
/// link whose policy is `error`, stat the destination and, using the shared
/// ownership predicate against the prior ledger, record a conflict when a
/// foreign file already sits there. An omni-owned destination (matching source,
/// whichever pin) is never a conflict, so a routine re-sync stays a no-op.
pub async fn existing_file_conflicts<S>(
    sys: &S,
    items: &[PlannedItem],
    prior_ledger: &Ledger,
) -> Result<Vec<ExistingConflict>>
where
    S: ApplierSys,
{
    let mut conflicts = Vec::new();
    for item in items {
        if item.on_existing != ExistingPolicy::Error {
            continue;
        }

        let dest_exists =
            symlink_meta(sys, &item.pair.dest_abs).await?.is_some();

        let prior = prior_ledger
            .links()
            .iter()
            .find(|l| l.source_id == item.source_id && l.dest == item.dest_rel);
        let prior_link = prior.map(|l| PriorLink {
            source_abs: Path::new(&l.target),
            pin_unchanged: true,
        });

        if is_foreign_existing(
            dest_exists,
            &item.pair.source_abs,
            prior_link.as_ref(),
        ) {
            conflicts.push(ExistingConflict {
                dest: item.pair.dest_abs.clone(),
            });
        }
    }
    Ok(conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::LinkPair;
    use omni_projection_configurations::{ExistingPolicy, LinkKind};

    fn item(
        source: &str,
        dest: &str,
        is_dir_link: bool,
        link: LinkKind,
        on_existing: ExistingPolicy,
    ) -> PlannedItem {
        PlannedItem {
            source_id: "src".to_string(),
            pair: LinkPair {
                source_abs: PathBuf::from(source),
                dest_abs: PathBuf::from(dest),
            },
            dest_rel: dest.trim_start_matches('/').to_string(),
            is_dir_link,
            on_existing,
            link,
            pin: "pin".to_string(),
        }
    }

    #[test]
    fn two_distinct_links_to_one_dest_are_flagged_even_under_overwrite() {
        let items = [
            item(
                "/src/a/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Overwrite,
            ),
            item(
                "/src/b/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Overwrite,
            ),
        ];
        let (dups, nested) = collision_conflicts(&items);
        assert_eq!(dups.len(), 1);
        assert!(nested.is_empty());
        assert_eq!(dups[0].dest, PathBuf::from("/ws/out/x"));
        assert_eq!(dups[0].sources.len(), 2);
    }

    #[test]
    fn links_from_different_sources_are_reported_together() {
        let items = [
            item(
                "/src/one/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
            item(
                "/src/two/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
        ];
        let (dups, _) = collision_conflicts(&items);
        assert_eq!(dups.len(), 1);
        assert!(dups[0].sources.contains(&PathBuf::from("/src/one/x")));
        assert!(dups[0].sources.contains(&PathBuf::from("/src/two/x")));
    }

    #[test]
    fn dest_nested_in_another_dir_link_is_flagged() {
        let items = [
            item(
                "/src/a/dir",
                "/ws/out/dir",
                true,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
            item(
                "/src/b/file",
                "/ws/out/dir/inner",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
        ];
        let (dups, nested) = collision_conflicts(&items);
        assert!(dups.is_empty());
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].inner, PathBuf::from("/ws/out/dir/inner"));
        assert_eq!(nested[0].outer, PathBuf::from("/ws/out/dir"));
    }

    #[test]
    fn identical_links_collapse_and_are_not_flagged() {
        let items = [
            item(
                "/src/a/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
            item(
                "/src/a/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
        ];
        let (dups, nested) = collision_conflicts(&items);
        assert!(dups.is_empty(), "identical links collapse");
        assert!(nested.is_empty());
    }

    #[test]
    fn a_dir_link_is_not_nested_under_itself() {
        let items = [item(
            "/src/a/dir",
            "/ws/out/dir",
            true,
            LinkKind::Auto,
            ExistingPolicy::Error,
        )];
        let (dups, nested) = collision_conflicts(&items);
        assert!(dups.is_empty());
        assert!(nested.is_empty());
    }

    #[test]
    fn a_plan_with_no_duplicates_produces_no_conflicts() {
        let items = [
            item(
                "/src/a/x",
                "/ws/out/x",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
            item(
                "/src/a/y",
                "/ws/out/y",
                false,
                LinkKind::Auto,
                ExistingPolicy::Error,
            ),
        ];
        let (dups, nested) = collision_conflicts(&items);
        assert!(dups.is_empty());
        assert!(nested.is_empty());
    }

    #[test]
    fn intra_projection_nesting_is_pruned_before_the_check_sees_it() {
        use crate::routing::{PlanInput, ScannedEntry, plan};

        let proj: omni_projection_configurations::Projection =
            serde_json::from_str(
                r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkgs/**","match_kind":"dir","dest":"@target/{path}/{basename}"}]}"#,
            )
            .unwrap();
        let entries = [
            ScannedEntry::dir("pkgs"),
            ScannedEntry::dir("pkgs/a"),
            ScannedEntry::dir("pkgs/a/nested"),
        ];
        let planned = plan(&PlanInput {
            workspace_root: Path::new("/ws"),
            source_root: Path::new("/src"),
            source_id: "id",
            projection: &proj,
            entries: &entries,
            env_files: &[],
        })
        .unwrap();

        let items: Vec<PlannedItem> = planned
            .into_iter()
            .map(|p| PlannedItem {
                source_id: "id".to_string(),
                dest_rel: p.pair.dest_abs.to_string_lossy().into_owned(),
                pair: p.pair,
                is_dir_link: p.is_dir_link,
                on_existing: ExistingPolicy::Error,
                link: LinkKind::Auto,
                pin: "pin".to_string(),
            })
            .collect();

        let (dups, nested) = collision_conflicts(&items);
        assert!(dups.is_empty());
        assert!(
            nested.is_empty(),
            "shallowest-wins pruning already removed the nested pair"
        );
    }

    use crate::apply::ResolvedKind;
    use crate::ledger::{Ledger, LedgerLink};
    use system_traits::impls::RealSys;

    fn real_item(source: &Path, dest: &Path, dest_rel: &str) -> PlannedItem {
        PlannedItem {
            source_id: "pkg".to_string(),
            pair: LinkPair {
                source_abs: source.to_path_buf(),
                dest_abs: dest.to_path_buf(),
            },
            dest_rel: dest_rel.to_string(),
            is_dir_link: false,
            on_existing: ExistingPolicy::Error,
            link: LinkKind::Auto,
            pin: "pin".to_string(),
        }
    }

    #[tokio::test]
    async fn foreign_existing_file_under_error_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src.txt");
        std::fs::write(&source, b"s").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"foreign").unwrap();

        let items = [real_item(&source, &dest, "dest.txt")];
        let conflicts =
            existing_file_conflicts(&RealSys, &items, &Ledger::default())
                .await
                .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].dest, dest);
    }

    #[tokio::test]
    async fn owned_unchanged_pin_dest_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src.txt");
        std::fs::write(&source, b"s").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"owned").unwrap();

        let ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "pkg".to_string(),
            dest: "dest.txt".to_string(),
            target: source.to_string_lossy().into_owned(),
            kind: ResolvedKind::Symlink,
            source_pin: "pin".to_string(),
            backup: None,
        }]);

        let items = [real_item(&source, &dest, "dest.txt")];
        let conflicts = existing_file_conflicts(&RealSys, &items, &ledger)
            .await
            .unwrap();
        assert!(conflicts.is_empty(), "re-sync of an owned dest is a no-op");
    }

    #[tokio::test]
    async fn owned_changed_pin_dest_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src.txt");
        std::fs::write(&source, b"s").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"owned").unwrap();

        let ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "pkg".to_string(),
            dest: "dest.txt".to_string(),
            target: source.to_string_lossy().into_owned(),
            kind: ResolvedKind::Symlink,
            source_pin: "old-pin".to_string(),
            backup: None,
        }]);

        let items = [real_item(&source, &dest, "dest.txt")];
        let conflicts = existing_file_conflicts(&RealSys, &items, &ledger)
            .await
            .unwrap();
        assert!(
            conflicts.is_empty(),
            "an owned dest is re-pointed, not treated as a conflict"
        );
    }
}
