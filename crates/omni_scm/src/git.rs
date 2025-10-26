use std::path::PathBuf;

use derive_new::new;
use git2::Repository;

use crate::{Scm, error::Error};

#[derive(new)]
pub struct Git {
    repo: Repository,
}

impl Scm for Git {
    #[inline(always)]
    fn changed_files(
        &self,
        base: &str,
        target: &str,
    ) -> Result<Vec<PathBuf>, Error> {
        let base = self.repo.revparse_single(base)?.peel_to_commit()?;
        let target = self.repo.revparse_single(target)?.peel_to_commit()?;
        let diff = self.repo.diff_tree_to_tree(
            Some(&base.tree()?),
            Some(&target.tree()?),
            None,
        )?;

        Ok(diff
            .deltas()
            .filter_map(|d| Some(d.new_file().path()?.to_owned()))
            .collect::<Vec<_>>())
    }

    #[inline(always)]
    fn default_base(&self) -> &str {
        "HEAD~1"
    }

    #[inline(always)]
    fn default_target(&self) -> &str {
        "HEAD"
    }
}
