use std::path::{Path, PathBuf};

use dir_walker::{
    DirEntry, DirWalker,
    impls::{RealGlobDirWalker, RealGlobDirWalkerBuilderError},
};
use omni_hasher::{Hasher, impls::Blake3Hasher};
use omni_types::{OmniPath, Root, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCanonicalizeAsync as _, FsCreateDirAllAsync as _, FsHardLinkAsync as _,
    FsMetadataAsync, FsReadAsync as _, FsRemoveDirAllAsync as _,
    FsWriteAsync as _, impls::RealSys,
};

use crate::{
    CachedFileOutput, CachedOutput, ProjectInfo, TaskOutputCacheStore,
    hash::{
        ProjectDirHasher,
        impls::{RealDirHasher, RealDirHasherError},
    },
    utils::{project_dirname, relpath, topmost_dir},
};

#[derive(Clone, Debug)]
pub struct LocalTaskOutputCacheStore {
    sys: RealSys,
    hasher: RealDirHasher,
    cache_dir: PathBuf,
    ws_root_dir: PathBuf,
}

impl LocalTaskOutputCacheStore {
    pub fn new(
        cache_dir: impl Into<PathBuf>,
        ws_root_dir: impl Into<PathBuf>,
    ) -> Self {
        let dir = cache_dir.into();
        let ws_root_dir = ws_root_dir.into();
        Self {
            sys: RealSys,
            hasher: RealDirHasher::builder()
                .standard_ignore_files(true)
                .custom_ignore_files(vec![".omniignore".to_string()])
                .workspace_root_dir(ws_root_dir.clone())
                .dir(dir.clone())
                .build()
                .expect("failed to build hasher"),
            cache_dir: dir,
            ws_root_dir,
        }
    }
}

impl LocalTaskOutputCacheStore {
    fn get_project_dir(&self, project_name: &str) -> PathBuf {
        let name = project_dirname(project_name);

        self.cache_dir.join(name).join("output")
    }

    async fn get_output_dir(
        &self,
        project: &ProjectInfo<'_>,
    ) -> Result<PathBuf, LocalTaskOutputCacheStoreError> {
        let hash = self.get_hash(project).await?;
        let proj_dir = self.get_project_dir(project.name);
        let output_dir = proj_dir.join(hash);

        Ok(output_dir)
    }

    async fn get_hash(
        &self,
        project: &ProjectInfo<'_>,
    ) -> Result<String, LocalTaskOutputCacheStoreError> {
        let mut tree = self
            .hasher
            .hash_tree::<Blake3Hasher>(project.name, project.dir, project.files)
            .await?;

        if !project.env_vars.is_empty() {
            let mut buff = vec![];
            for env_key in project.env_cache_keys {
                let value = project
                    .env_vars
                    .get(env_key)
                    .map(|s| s.as_str())
                    .unwrap_or("");

                buff.push(format!("{env_key}={value}"));
            }

            let env_vars = buff.join("\n");

            tree.insert(Blake3Hasher::hash(env_vars.as_bytes()));
        }

        tree.commit();

        Ok(
            bs58::encode(tree.root().expect("unable to get root"))
                .into_string(),
        )
    }
}

const LOGS_CACHE_FILE: &str = "logs.cache";
const CACHE_OUTPUT_METADATA_FILE: &str = "cache.meta.bin";

#[async_trait::async_trait]
impl TaskOutputCacheStore for LocalTaskOutputCacheStore {
    type Error = LocalTaskOutputCacheStoreError;

    async fn cache(
        &self,
        project: &ProjectInfo,
        logs: Option<&str>,
    ) -> Result<(), Self::Error> {
        let cache_output_dir = self.get_output_dir(project).await?;

        // we don't want any old files in the cache, edge case is if the user
        // has changed the name of the project, but the cache is still there
        if self.sys.fs_exists_async(&cache_output_dir).await? {
            self.sys.fs_remove_dir_all_async(&cache_output_dir).await?;
        }

        self.sys.fs_create_dir_all_async(&cache_output_dir).await?;
        let mut logs_path_abs: Option<_> = None;
        if let Some(logs) = logs {
            let log_path = cache_output_dir.join(LOGS_CACHE_FILE);
            self.sys.fs_write_async(&log_path, logs).await?;

            logs_path_abs = Some(log_path);
        }

        let bases = enum_map! {
            Root::Workspace => &self.ws_root_dir,
            Root::Project => project.dir,
        };

        let mut includes = vec![];
        for p in project.output_files {
            let p = p.resolve(&bases);

            let path = if p.is_relative() {
                std::path::absolute(project.dir.join(p))
                    .expect("it should be absolute")
            } else {
                p.to_path_buf()
            };

            includes.push(path);
        }

        let topmost = topmost_dir(
            self.sys.clone(),
            &includes,
            &self.ws_root_dir,
            project.dir,
        )
        .to_path_buf();

        let dirwalker = RealGlobDirWalker::builder()
            .custom_ignore_filenames(vec![".omniignore".to_string()])
            .include(includes)
            .build()?;

        let mut cached_files = vec![];

        for res in dirwalker.walk_dir(&[&topmost])? {
            let res = res?;
            let original_file_abs_path = res.path();
            let original_omni_path = if original_file_abs_path
                .starts_with(project.dir)
            {
                OmniPath::new_project_rooted(relpath(
                    original_file_abs_path,
                    project.dir,
                ))
            } else if original_file_abs_path.starts_with(&self.ws_root_dir) {
                OmniPath::new_ws_rooted(relpath(
                    original_file_abs_path,
                    &self.ws_root_dir,
                ))
            } else {
                OmniPath::new(original_file_abs_path)
            };

            let encoded =
                bs58::encode(original_omni_path.to_string().as_bytes())
                    .into_string();
            let cache_abs_path =
                cache_output_dir.join(format!("{encoded}.cache"));
            let cached_rel_path = relpath(&cache_abs_path, &cache_output_dir);

            // TODO: check if there's faster way to do this
            self.sys
                .fs_hard_link_async(original_file_abs_path, &cache_abs_path)
                .await?;

            cached_files.push(CachedFileOutput {
                cached_path: cached_rel_path.to_path_buf(),
                original_path: original_omni_path,
            });
        }
        let metadata_path = cache_output_dir.join(CACHE_OUTPUT_METADATA_FILE);
        let metadata = CachedOutput {
            logs_path: logs_path_abs
                .map(|p| relpath(&p, &cache_output_dir).to_path_buf()),
            files: cached_files,
        };
        let bytes = bincode::serde::encode_to_vec(
            &metadata,
            bincode::config::standard(),
        )?;

        self.sys.fs_write_async(&metadata_path, &bytes).await?;

        Ok(())
    }

    async fn get(
        &self,
        project: &ProjectInfo,
    ) -> Result<Option<CachedOutput>, Self::Error> {
        let output_dir = self.get_output_dir(project).await?;
        let file = output_dir.join(CACHE_OUTPUT_METADATA_FILE);

        let cache_abs = |p: &Path| std::path::absolute(output_dir.join(p));

        if self.sys.fs_exists_async(&file).await? {
            let bytes = self.sys.fs_read_async(&file).await?;
            let (mut cached_output, _): (CachedOutput, _) =
                bincode::serde::decode_from_slice(
                    &bytes,
                    bincode::config::standard(),
                )?;

            // canonicalize the paths
            if let Some(logs_path) = cached_output.logs_path.as_mut() {
                let p = cache_abs(logs_path)?;

                if !self.sys.fs_exists_async(&p).await? {
                    return Ok(None);
                }

                *logs_path = self.sys.fs_canonicalize_async(p).await?;
            }

            for file in cached_output.files.iter_mut() {
                let c = cache_abs(&file.cached_path)?;

                if !self.sys.fs_exists_async(&c).await? {
                    return Ok(None);
                }

                file.cached_path = self.sys.fs_canonicalize_async(c).await?;
            }

            Ok(Some(cached_output))
        } else {
            Ok(None)
        }
    }

    async fn invalidate_caches(
        &self,
        project_name: &str,
    ) -> Result<(), Self::Error> {
        let path = self.get_project_dir(project_name);

        self.sys.fs_remove_dir_all_async(path).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct LocalTaskOutputCacheStoreError {
    kind: LocalTaskOutputCacheStoreErrorKind,
    #[source]
    inner: LocalTaskOutputCacheStoreErrorInner,
}

impl<T: Into<LocalTaskOutputCacheStoreErrorInner>> From<T>
    for LocalTaskOutputCacheStoreError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(LocalTaskOutputCacheStoreErrorKind), vis(pub))]
enum LocalTaskOutputCacheStoreErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    DirHasher(#[from] RealDirHasherError),

    #[error(transparent)]
    RealGlobDirWalkerBuilder(#[from] RealGlobDirWalkerBuilderError),

    #[error(transparent)]
    Ignore(#[from] dir_walker::impls::IgnoreError),

    #[error(transparent)]
    IgnoreBuild(#[from] dir_walker::impls::IgnoreRealDirWalkerError),

    #[error(transparent)]
    Encode(#[from] bincode::error::EncodeError),

    #[error(transparent)]
    Decode(#[from] bincode::error::DecodeError),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::cache::impls::LocalTaskOutputCacheStore;
    use derive_new::new;
    use system_traits::{FsRename, impls::RealSys};
    use tokio::io::AsyncReadExt as _;
    use yoke::Yoke;

    const JS_CONTENT: &str = include_str!("../../../test_fixtures/test.js");
    const TXT_CONTENT: &str = include_str!("../../../test_fixtures/test.txt");
    const LOGS_CONTENT: &str = include_str!("../../../test_fixtures/logs.txt");

    fn sys() -> RealSys {
        RealSys
    }

    async fn write_project(dir: &Path, sys: RealSys) {
        sys.fs_create_dir_all_async(dir.join("src"))
            .await
            .expect("failed to create project1 src dir");
        sys.fs_create_dir_all_async(dir.join("dist"))
            .await
            .expect("failed to create project1 dist dir");

        sys.fs_write_async(dir.join("src/a-test.txt"), TXT_CONTENT)
            .await
            .expect("failed to write test file");
        sys.fs_write_async(dir.join("src/b-test.txt"), TXT_CONTENT)
            .await
            .expect("failed to write test file");
        sys.fs_write_async(dir.join("dist/a-test.js"), JS_CONTENT)
            .await
            .expect("failed to write test file");
        sys.fs_write_async(dir.join("dist/b-test.js"), JS_CONTENT)
            .await
            .expect("failed to write test file");
    }

    async fn fixture(projects: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let root = dir.path();
        let sys = sys();
        let cache_dir = dir.path().join(".omni/cache");

        sys.fs_create_dir_all_async(cache_dir)
            .await
            .expect("failed to create cache dir");

        for project in projects {
            let project_dir = root.join(project);
            write_project(&project_dir, sys.clone()).await;
        }

        dir
    }

    fn env_vars() -> maps::Map<String, String> {
        maps::map![
            "KEY".to_string() => "value".to_string()
        ]
    }

    fn env_cache_keys() -> Vec<String> {
        vec!["KEY".to_string()]
    }

    #[derive(new)]
    struct ProjectInfoStatic {
        name: String,
        dir: PathBuf,
        files: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        env_vars: maps::Map<String, String>,
        env_cache_keys: Vec<String>,
    }

    fn project_from_static<'a>(
        project: &'a ProjectInfoStatic,
    ) -> ProjectInfo<'a> {
        ProjectInfo::new(
            &project.name,
            &project.dir,
            &project.output_files,
            &project.files,
            &project.env_vars,
            &project.env_cache_keys,
        )
    }

    fn project_with_mut(
        name: &str,
        root_dir: &Path,
        mut f: impl FnMut(&mut ProjectInfoStatic),
    ) -> Yoke<ProjectInfo<'static>, Box<ProjectInfoStatic>> {
        let project_dir = root_dir.join(name);
        let mut owned = ProjectInfoStatic {
            name: name.to_string(),
            files: vec![OmniPath::new("src/**/*.txt")],
            output_files: vec![OmniPath::new("dist/**/*.js")],
            dir: project_dir,
            env_vars: env_vars(),
            env_cache_keys: env_cache_keys(),
        };
        f(&mut owned);

        Yoke::attach_to_cart(Box::new(owned), |owned| {
            project_from_static(owned)
        })
    }

    #[inline(always)]
    fn project(
        name: &str,
        root_dir: &Path,
    ) -> Yoke<ProjectInfo<'static>, Box<ProjectInfoStatic>> {
        project_with_mut(name, root_dir, |_| {})
    }

    fn cache_store(root: &Path) -> LocalTaskOutputCacheStore {
        LocalTaskOutputCacheStore::new(root.join(".omni/cache"), root)
    }

    async fn read_string(path: &Path) -> String {
        let mut file = tokio::fs::File::open(path)
            .await
            .expect("failed to open file");
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .await
            .expect("failed to read file");

        contents
    }

    async fn read_bytes(path: &Path) -> Vec<u8> {
        let mut file = tokio::fs::File::open(path)
            .await
            .expect("failed to open file");
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .await
            .expect("failed to read file");

        contents
    }

    #[tokio::test]
    async fn test_cache_unchanged() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);

        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_files_changed_after_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);
        let sys = sys();

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        sys.fs_write_async(
            project.get().dir.join("src/a-test.txt"),
            "new content",
        )
        .await
        .expect("failed to write file");

        let cached_output1 = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        // recache then check
        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output2 = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_none(), "cached output should not exist");
        assert!(cached_output2.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_logs_path_should_not_exist_if_no_logs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), None)
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(
            cached_output.is_some(),
            "cached output should exist even if no logs"
        );
        assert!(
            cached_output.unwrap().logs_path.is_none(),
            "logs path should not exist if no logs"
        );
    }

    #[tokio::test]
    async fn test_output_should_return_none_if_cached_file_is_deleted() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        // delete a cached file
        let cached_file = &cached_output.files[0];
        tokio::fs::remove_file(&cached_file.cached_path)
            .await
            .expect("failed to delete file");

        let cached_output2 = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(
            cached_output2.is_none(),
            "cached output should not exist if cached file is deleted"
        );
    }

    #[tokio::test]
    async fn test_env_changed_after_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let mut project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        project = project_with_mut("project1", dir, |project| {
            project
                .env_vars
                .insert("KEY".to_string(), "value-changed".to_string());
        });

        let cached_output1 = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        // recache then check
        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output2 = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(
            cached_output1.is_none(),
            "cached output should not exist if env changed"
        );
        assert!(
            cached_output2.is_some(),
            "cached output should exist if recached with new env"
        );
    }

    #[tokio::test]
    async fn test_cached_log_content() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        assert_eq!(
            read_string(
                &cached_output.logs_path.expect("logs path should exist")
            )
            .await,
            LOGS_CONTENT,
            "logs content should match"
        );
    }

    #[tokio::test]
    async fn test_cached_file_content() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        for file in cached_output.files.iter() {
            assert_eq!(
                read_bytes(&file.cached_path).await,
                JS_CONTENT.as_bytes(),
                "file content should match {} and {}",
                file.cached_path.display(),
                file.original_path
            );
        }
    }

    #[tokio::test]
    async fn test_moving_project_folder_should_not_invalidate_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        tokio::fs::rename(project.get().dir, dir.join("project1-renamed"))
            .await
            .expect("failed to rename");

        let renamed_project_folder = project_with_mut("project1", dir, |p| {
            p.dir = dir.join("project1-renamed");
        });

        let cached_output = cache
            .get(renamed_project_folder.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_renaming_a_file_should_invalidate_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project("project1", dir);

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(
                project.get().dir.join("src/a-test.txt"),
                project.get().dir.join("src/a-test-renamed.txt"),
            )
            .expect("failed to rename");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_use_rooted_omni_path() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project = project_with_mut("project1", dir, |p| {
            p.output_files = vec![
                OmniPath::new_ws_rooted("rootfile.txt"),
                OmniPath::new_project_rooted("dist/**/*.js"),
            ];
        });

        // Add a file in the workspace root
        sys()
            .fs_write_async(dir.join("rootfile.txt"), "root file content")
            .await
            .expect("failed to write file");

        cache
            .cache(project.get(), Some(LOGS_CONTENT))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(project.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        assert_eq!(cached_output.files.len(), 3, "should be 3 files");
        assert_eq!(
            cached_output.files[0].original_path.to_string(),
            "@workspace/rootfile.txt",
        );
        assert_eq!(
            cached_output.files[1].original_path.to_string(),
            "@project/dist/b-test.js",
        );
        assert_eq!(
            cached_output.files[2].original_path.to_string(),
            "@project/dist/a-test.js",
        );
    }
}
