use std::path::PathBuf;

use omni_lockfile::Lockfile;
use url::Url;

use crate::{
    error::Error, manager::config::RemoteSourceConfig, sys::RemoteSourceSys,
};

pub struct RemoteSourceManager<TSys: RemoteSourceSys> {
    lockfile: Lockfile,
    source_dir_path: PathBuf,
    sys: TSys,
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    pub async fn new(
        config: RemoteSourceConfig,
        sys: TSys,
    ) -> Result<RemoteSourceManager<TSys>, Error> {
        let lockfile = Lockfile::load(config.lockfile_path, &sys).await?;

        Ok(RemoteSourceManager {
            lockfile,
            sys,
            source_dir_path: config.soure_dir_path,
        })
    }
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    pub async fn pull_git_repo(
        &self,
        uri: &Url,
        rev: &str,
    ) -> Result<PathBuf, Error> {
        let commit = self.lockfile.get_git_commit(uri, rev).await;
        let slug = omni_git_utils::url_to_safe_dir_name(uri.as_str())?;
        let dst = self.source_dir_path.join("git").join(slug).join(rev);

        if !self.sys.fs_exists_no_err_async(&dst).await {
            self.sys.fs_create_dir_all_async(&dst).await?;
            log::trace!("created dir: {dst:?}");

            let clone = omni_git_utils::clone_repo(
                &self.sys,
                uri.as_str(),
                Some(commit.as_deref().unwrap_or(rev)),
                &dst,
            )
            .await?;
            log::trace!(
                "cloned git repo uri: {}, rev: {}",
                uri,
                commit.as_deref().unwrap_or(rev),
            );

            if commit.is_none() {
                self.lockfile
                    .lock_git_commit(uri, rev, &clone.commit)
                    .await?;
            }
        }

        Ok(dst)
    }

    pub async fn lock(&self) -> Result<(), Error> {
        self.lockfile.save(&self.sys).await?;
        Ok(())
    }
}
