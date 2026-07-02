use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use derive_new::new;
use maps::map;
use tokio::sync::Mutex;
use trace::Level;
use url::Url;

use crate::{
    LockfileSys,
    error::Error,
    lockfile_data::{GitRepoLockData, LockfileData},
};

#[derive(new)]
pub struct Lockfile {
    data: Mutex<LockfileData>,
    is_modified: AtomicBool,
    path: PathBuf,
}

impl Lockfile {
    pub async fn load(
        file: impl Into<PathBuf>,
        sys: &impl LockfileSys,
    ) -> Result<Self, Error> {
        let file = file.into();
        if sys.fs_exists_no_err_async(file.as_path()).await {
            if !sys.fs_is_file_no_err_async(file.as_path()).await {
                return Err(eyre::eyre!(
                    "path exists but is not a file {}",
                    file.display()
                )
                .into());
            }

            let data = omni_file_data_serde::read_async::<LockfileData, _, _>(
                &file, sys,
            )
            .await?;
            Ok(Lockfile::new(
                Mutex::new(data),
                AtomicBool::new(false),
                file,
            ))
        } else {
            Ok(Lockfile::new(
                Mutex::new(LockfileData::default()),
                AtomicBool::new(false),
                file,
            ))
        }
    }
}

impl Lockfile {
    pub async fn modify(
        &self,
        updater_fn: impl FnOnce(&mut LockfileData) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut data = self.data.lock().await;
        (updater_fn)(&mut data)?;

        self.is_modified.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip_all)
    )]
    pub async fn lock_git_commit(
        &self,
        uri: &Url,
        rev: &str,
        commit: &str,
    ) -> Result<(), Error> {
        let uri = uri.clone();
        let rev = rev.to_string();
        let commit = commit.to_string();

        log::trace!("locking git repo: {uri}, rev: {rev}, commit: {commit}");
        self.modify(|d| {
            match d {
                LockfileData::V1_0_0(v1) => {
                    let repo = v1.git.get_mut(&uri);

                    if let Some(repo) = repo {
                        if let Some(rev) = repo.get_mut(&rev) {
                            rev.commit = commit;
                        } else {
                            repo.insert(rev, GitRepoLockData::new(commit));
                        }
                    } else {
                        v1.git.insert(
                            uri,
                            map! {
                                rev => GitRepoLockData::new(commit)
                            },
                        );
                    }
                }
            }

            Ok(())
        })
        .await?;

        log::trace!("lock successful");

        Ok(())
    }

    pub async fn get_git_commit(&self, uri: &Url, rev: &str) -> Option<String> {
        match &*self.data.lock().await {
            LockfileData::V1_0_0(v1) => v1
                .git
                .get(uri)
                .and_then(|r| r.get(rev).map(|r| r.commit.clone())),
        }
    }

    pub async fn save(&self, sys: &impl LockfileSys) -> Result<(), Error> {
        let is_modified = self.is_modified.load(Ordering::Relaxed);
        if is_modified {
            let data = self.data.lock().await;
            omni_file_data_serde::write_async(self.path.as_path(), &*data, sys)
                .await?;
        }
        Ok(())
    }
}
