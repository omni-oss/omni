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
    lockfile_data::{GitRepoLockData, LockfileData, LockfileDataV1_0_0},
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

    /// A snapshot of every pinned git source as `(uri, rev, commit)` tuples.
    pub async fn git_pins(&self) -> Vec<(Url, String, String)> {
        match &*self.data.lock().await {
            LockfileData::V1_0_0(v1) => v1
                .git
                .iter()
                .flat_map(|(uri, revs)| {
                    revs.iter().map(move |(rev, data)| {
                        (uri.clone(), rev.clone(), data.commit.clone())
                    })
                })
                .collect(),
        }
    }

    pub async fn save(&self, sys: &impl LockfileSys) -> Result<(), Error> {
        let is_modified = self.is_modified.load(Ordering::Relaxed);
        if is_modified {
            let data = self.data.lock().await;
            let sorted = sorted_view(&data);
            omni_file_data_serde::write_async(
                self.path.as_path(),
                &sorted,
                sys,
            )
            .await?;
        }
        Ok(())
    }
}

fn sorted_view(data: &LockfileData) -> LockfileData {
    match data {
        LockfileData::V1_0_0(v1) => {
            let mut git = v1.git.clone();
            for revs in git.values_mut() {
                revs.sort_unstable_keys();
            }
            git.sort_unstable_keys();
            LockfileData::V1_0_0(LockfileDataV1_0_0 { git })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys_of(data: &LockfileData) -> Vec<(String, Vec<String>)> {
        match data {
            LockfileData::V1_0_0(v1) => v1
                .git
                .iter()
                .map(|(uri, revs)| {
                    (uri.to_string(), revs.keys().cloned().collect::<Vec<_>>())
                })
                .collect(),
        }
    }

    #[test]
    fn sorted_view_is_stable_across_insertion_orders() {
        let a = Url::parse("https://example.com/a.git").unwrap();
        let b = Url::parse("https://example.com/b.git").unwrap();

        let forward = LockfileData::V1_0_0(LockfileDataV1_0_0 {
            git: map! {
                a.clone() => map! {
                    "main".to_string() => GitRepoLockData::new("c1"),
                    "dev".to_string() => GitRepoLockData::new("c2"),
                },
                b.clone() => map! {
                    "v2".to_string() => GitRepoLockData::new("c3"),
                    "v1".to_string() => GitRepoLockData::new("c4"),
                },
            },
        });

        let reversed = LockfileData::V1_0_0(LockfileDataV1_0_0 {
            git: map! {
                b => map! {
                    "v1".to_string() => GitRepoLockData::new("c4"),
                    "v2".to_string() => GitRepoLockData::new("c3"),
                },
                a => map! {
                    "dev".to_string() => GitRepoLockData::new("c2"),
                    "main".to_string() => GitRepoLockData::new("c1"),
                },
            },
        });

        assert_eq!(
            keys_of(&sorted_view(&forward)),
            keys_of(&sorted_view(&reversed))
        );
        assert_eq!(
            keys_of(&sorted_view(&forward)),
            vec![
                (
                    "https://example.com/a.git".to_string(),
                    vec!["dev".to_string(), "main".to_string()]
                ),
                (
                    "https://example.com/b.git".to_string(),
                    vec!["v1".to_string(), "v2".to_string()]
                ),
            ]
        );
    }
}
