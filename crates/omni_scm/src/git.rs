use std::path::PathBuf;

use derive_new::new;
use gix::{Repository, bstr::ByteSlice};

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
        log::trace!("getting changed files between {} and {}", base, target);
        let base = get_tree_from_spec(&self.repo, base)?;
        let target = get_tree_from_spec(&self.repo, target)?;

        let diff = self
            .repo
            .diff_tree_to_tree(Some(&base), Some(&target), None)
            .map_err(gix::Error::from_error)?;
        let diff = diff
            .iter()
            .filter_map(|entry| {
                entry.location().to_path().map(|p| p.to_path_buf()).ok()
            })
            .collect();
        log::trace!("git diff {}..{}: {:#?}", base.id(), target.id(), diff);
        Ok(diff)
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

fn get_tree_from_spec<'a>(
    repo: &'a Repository,
    spec: &str,
) -> Result<gix::Tree<'a>, gix::Error> {
    repo.rev_parse_single(spec)
        .map_err(gix::Error::from_error)?
        .object()
        .map_err(gix::Error::from_error)?
        .into_commit()
        .tree()
        .map_err(gix::Error::from_error)
}
