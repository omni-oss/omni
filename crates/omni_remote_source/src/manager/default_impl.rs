use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use omni_git_utils::CloneInfo;
use omni_lockfile::{Lockfile, data::LockfileData};
use tokio::task::JoinSet;
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
        let dest = self.git_dest_dir(uri, Some(rev))?;

        match commit {
            Some(commit) => {
                if !self.sys.fs_exists_no_err_async(&dest).await {
                    log::trace!("created dir: {dest:?}");
                    self.sys.fs_create_dir_all_async(&dest).await?;
                    self.clone_repo_inner(&dest, uri, &commit).await?;
                }
            }
            None => {
                if self.sys.fs_exists_no_err_async(&dest).await {
                    log::trace!("removing dir: {dest:?}");
                    self.sys.fs_remove_dir_all_async(&dest).await?;
                }
                log::trace!("created dir: {dest:?}");
                self.sys.fs_create_dir_all_async(&dest).await?;

                let clone = self
                    .clone_repo_inner(
                        &dest,
                        uri,
                        commit.as_deref().unwrap_or(rev),
                    )
                    .await?;

                self.lockfile
                    .lock_git_commit(uri, rev, &clone.commit)
                    .await?;
            }
        }

        Ok(dest)
    }

    pub async fn retain_git_sources(
        &self,
        git_sources: &[(&Url, &str)],
    ) -> Result<(), Error> {
        let mut source_map = HashMap::<&Url, HashSet<&str>>::new();

        for (url, rev) in git_sources {
            if let Some(revs) = source_map.get_mut(url) {
                revs.insert(*rev);
            } else {
                let set = HashSet::from_iter([*rev]);

                source_map.insert(*url, set);
            }
        }

        let mut rm_dirs: Vec<(Url, Option<String>)> = vec![];

        self.lockfile
            .modify(|d| {
                match d {
                    LockfileData::V1_0_0(v1) => {
                        v1.git.retain(|k, v| {
                            if !source_map.contains_key(k) {
                                rm_dirs.push((k.clone(), None));
                                return false;
                            }

                            let revs =
                                source_map.get(k).expect("should have value");

                            v.retain(|x, _| {
                                let should_retain = revs.contains(x.as_str());

                                if !should_retain {
                                    rm_dirs.push((k.clone(), Some(x.clone())));
                                }

                                should_retain
                            });

                            if v.is_empty() {
                                rm_dirs.push((k.clone(), None));
                                return false;
                            }

                            return true;
                        });
                    }
                }

                Ok(())
            })
            .await?;

        let mut rm_tasks = JoinSet::new();
        for (uri, rev) in rm_dirs {
            let dest = self.git_dest_dir(&uri, rev.as_deref())?;
            let sys = self.sys.clone();

            rm_tasks.spawn(async move {
                if sys.fs_exists_async(&dest).await? {
                    sys.fs_remove_dir_all_async(&dest).await?;
                    log::debug!("removed stale git source directory: {dest:?}");
                }

                Ok::<_, Error>(())
            });
        }

        for t in rm_tasks.join_all().await {
            t?;
        }

        Ok(())
    }

    pub async fn lock(&self) -> Result<(), Error> {
        self.lockfile.save(&self.sys).await?;
        Ok(())
    }

    async fn clone_repo_inner(
        &self,
        dest: &Path,
        uri: &Url,
        commit: &str,
    ) -> Result<CloneInfo, Error> {
        let clone = omni_git_utils::clone_repo(
            &self.sys,
            uri.as_str(),
            Some(&commit),
            dest,
        )
        .await?;
        log::trace!("cloned git repo uri: {}, rev: {}", uri, commit,);
        Ok(clone)
    }

    fn git_dest_dir(
        &self,
        uri: &Url,
        rev: Option<&str>,
    ) -> Result<PathBuf, omni_git_utils::Error> {
        let slug = omni_git_utils::url_to_safe_dir_name(uri.as_str())?;
        let path = self.source_dir_path.join("git").join(slug);

        Ok(if let Some(rev) = rev {
            path.join(rev)
        } else {
            path
        })
    }
}
