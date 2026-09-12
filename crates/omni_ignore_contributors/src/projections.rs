use std::path::{Path, PathBuf};

use omni_ignore_core::{IgnoreContributor, IgnorePattern};
use system_traits::FsReadAsync;

/// Emits one anchored ignore pattern per projection link recorded in the ledger.
/// The set always matches what a projection sync actually linked, since it is a
/// direct read of the ledger with no config resolution or filesystem walk.
pub struct ProjectionsContributor<S> {
    sys: S,
    ledger_path: PathBuf,
}

impl<S> ProjectionsContributor<S> {
    pub fn new(sys: S, ledger_path: PathBuf) -> Self {
        Self { sys, ledger_path }
    }
}

#[async_trait::async_trait]
impl<S> IgnoreContributor for ProjectionsContributor<S>
where
    S: FsReadAsync + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "projections"
    }

    async fn patterns(&self) -> eyre::Result<Vec<IgnorePattern>> {
        let ledger = omni_projections::load(&self.sys, &self.ledger_path).await;
        let mut dests: Vec<String> = ledger
            .links()
            .iter()
            .map(|link| link.dest.clone())
            .collect();
        dests.sort();
        Ok(dests
            .iter()
            .map(|dest| IgnorePattern::path(Path::new(dest)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use omni_projections::{Ledger, LedgerLink, ResolvedKind, save};
    use system_traits::{FsCreateDirAllAsync, impls::InMemorySys};

    use super::*;

    fn link(dest: &str) -> LedgerLink {
        LedgerLink {
            source_id: "src".to_string(),
            dest: dest.to_string(),
            target: "t".to_string(),
            kind: ResolvedKind::Symlink,
            source_pin: "pin".to_string(),
            backup: None,
        }
    }

    #[tokio::test]
    async fn emits_one_sorted_anchored_pattern_per_dest() {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all_async(Path::new("/ws")).await.unwrap();
        let ledger_path = PathBuf::from("/ws/links.json");
        let ledger = Ledger::from_links(vec![
            link(".agents/skills/unslop"),
            link(".agents/skills/bro"),
        ]);
        save(&sys, &ledger_path, &ledger).await.unwrap();

        let contributor = ProjectionsContributor::new(sys, ledger_path);
        let patterns = contributor.patterns().await.unwrap();
        let lines: Vec<&str> =
            patterns.iter().map(IgnorePattern::as_str).collect();

        assert_eq!(contributor.name(), "projections");
        assert_eq!(
            lines,
            vec!["/.agents/skills/bro", "/.agents/skills/unslop"]
        );
    }

    #[tokio::test]
    async fn a_missing_ledger_yields_no_patterns() {
        let sys = InMemorySys::default();
        let contributor =
            ProjectionsContributor::new(sys, PathBuf::from("/ws/links.json"));
        assert!(contributor.patterns().await.unwrap().is_empty());
    }
}
