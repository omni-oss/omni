use std::path::{Path, PathBuf};

use dir_walker::{
    DirEntry, DirWalker,
    impls::{RealGlobDirWalker, RealGlobDirWalkerBuilderError},
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use maps::Map;
use omni_hasher::{Hasher, impls::Blake3Hasher};
use omni_types::{OmniPath, Root, RootMap, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCanonicalizeAsync as _, FsCreateDirAllAsync, FsHardLinkAsync as _,
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
                .workspace_root_dir(ws_root_dir.clone())
                .dir(dir.clone())
                .build()
                .expect("failed to build hasher"),
            cache_dir: dir,
            ws_root_dir,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectResult<'a> {
    project: ProjectInfo<'a>,
    input_files: Option<Vec<OmniPath>>,
    output_files: Option<Vec<OmniPath>>,
    hash: Option<String>,
    cache_output_dir: Option<PathBuf>,
    roots: RootMap<'a>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct CollectConfig {
    pub output_files: bool,
    pub input_files: bool,
    pub hashes: bool,
    pub cache_output_dirs: bool,
}

struct HashInput<'a> {
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub input_files: &'a [OmniPath],
    pub input_env_cache_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
}

fn hashtext(text: &str) -> String {
    let x = Blake3Hasher::hash(text.as_bytes());
    bs58::encode(x).into_string()
}

impl LocalTaskOutputCacheStore {
    fn get_project_dir(&self, project_name: &str) -> PathBuf {
        let name = project_dirname(project_name);

        self.cache_dir.join(name).join("output")
    }

    async fn get_output_dir(
        &self,
        project_name: &str,
        hash: &str,
    ) -> Result<PathBuf, LocalTaskOutputCacheStoreError> {
        let proj_dir = self.get_project_dir(project_name);
        let output_dir = proj_dir.join(hash);

        Ok(output_dir)
    }

    async fn get_hash(
        &self,
        hash_input: &HashInput<'_>,
    ) -> Result<String, LocalTaskOutputCacheStoreError> {
        let mut tree = self
            .hasher
            .hash_tree::<Blake3Hasher>(
                hash_input.project_name,
                hash_input.project_dir,
                hash_input.input_files,
            )
            .await?;

        if !hash_input.env_vars.is_empty() {
            let mut buff = vec![];
            for env_key in hash_input.input_env_cache_keys {
                let value = hash_input
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

    async fn collect<'a>(
        &'a self,
        projects: &'a [ProjectInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, LocalTaskOutputCacheStoreError> {
        struct Holder<'a> {
            output_files_globset: Option<GlobSet>,
            resolved_output_files: Option<Vec<OmniPath>>,
            input_files_globset: Option<GlobSet>,
            resolved_input_files: Option<Vec<OmniPath>>,
            project: &'a ProjectInfo<'a>,
            roots: RootMap<'a>,
            hash: Option<String>,
            cache_output_dir: Option<PathBuf>,
        }

        let mut to_process = Vec::with_capacity(projects.len());

        // holds paths in input files and output files
        let mut includes = Vec::with_capacity(
            projects
                .iter()
                .map(|i| i.output_files.len() + i.input_files.len())
                .sum(),
        );

        for project in projects {
            let roots = enum_map! {
                Root::Workspace => &self.ws_root_dir,
                Root::Project => project.dir,
            };

            let mut output_files_globset = None;
            if config.output_files {
                let mut o = GlobSetBuilder::new();

                populate_includes_and_globset(
                    project.output_files,
                    project.dir,
                    &mut includes,
                    &roots,
                    &mut o,
                )?;

                output_files_globset = Some(o.build()?);
            }

            let mut input_files_globset = None;

            let should_collect_input_files =
                config.input_files || config.hashes || config.cache_output_dirs;
            if should_collect_input_files {
                let mut i = GlobSetBuilder::new();
                populate_includes_and_globset(
                    project.input_files,
                    project.dir,
                    &mut includes,
                    &roots,
                    &mut i,
                )?;

                input_files_globset = Some(i.build()?);
            }

            to_process.push(Holder {
                input_files_globset,
                output_files_globset,
                resolved_input_files: if should_collect_input_files {
                    // just in the collected input files will be the same as the
                    // original input files
                    Some(Vec::with_capacity(project.input_files.len()))
                } else {
                    None
                },
                resolved_output_files: if config.output_files {
                    // just in the collected output files will be the same as the
                    // original output files
                    Some(Vec::with_capacity(project.output_files.len()))
                } else {
                    None
                },
                project,
                roots,
                hash: None,
                cache_output_dir: None,
            });
        }

        if !includes.is_empty() {
            let topmost =
                topmost_dir(self.sys.clone(), &includes, &self.ws_root_dir)
                    .to_path_buf();

            let dirwalker = RealGlobDirWalker::builder()
                .custom_ignore_filenames(vec![".omniignore".to_string()])
                .include(includes)
                .build()?;

            for res in dirwalker.walk_dir(&[&topmost])? {
                let res = res?;
                let original_file_abs_path = res.path();
                for project in &mut to_process {
                    let project_dir = project.roots[Root::Project];
                    let rooted_path =
                        if original_file_abs_path.starts_with(project_dir) {
                            OmniPath::new_project_rooted(relpath(
                                original_file_abs_path,
                                project_dir,
                            ))
                        } else if original_file_abs_path
                            .starts_with(&self.ws_root_dir)
                        {
                            OmniPath::new_ws_rooted(relpath(
                                original_file_abs_path,
                                &self.ws_root_dir,
                            ))
                        } else {
                            OmniPath::new(original_file_abs_path)
                        };

                    if let Some(input_files_globset) =
                        project.input_files_globset.as_ref()
                        && input_files_globset.is_match(original_file_abs_path)
                        && let Some(resolved_input_files) =
                            project.resolved_input_files.as_mut()
                    {
                        resolved_input_files.push(rooted_path.clone());
                    }

                    if let Some(output_files_globset) =
                        project.output_files_globset.as_ref()
                        && output_files_globset.is_match(original_file_abs_path)
                        && let Some(resolved_output_files) =
                            project.resolved_output_files.as_mut()
                    {
                        resolved_output_files.push(rooted_path);
                    }
                }
            }
        }

        if config.hashes || config.cache_output_dirs {
            for holder in &mut to_process {
                let hash = self
                    .get_hash(&HashInput {
                        project_name: holder.project.name,
                        project_dir: holder.project.dir,
                        input_files: holder
                            .resolved_input_files
                            .as_ref()
                            .expect("should be some"),
                        input_env_cache_keys: holder
                            .project
                            .input_env_cache_keys,
                        env_vars: holder.project.env_vars,
                    })
                    .await?;

                holder.hash = Some(hash);
            }
        }

        if config.cache_output_dirs {
            for holder in &mut to_process {
                let output_dir = self
                    .get_output_dir(
                        holder.project.name,
                        holder.hash.as_ref().expect("should be some"),
                    )
                    .await?;

                holder.cache_output_dir = Some(output_dir);
            }
        }

        Ok(to_process
            .into_iter()
            .map(|p| CollectResult {
                project: *p.project,
                input_files: p.resolved_input_files,
                output_files: p.resolved_output_files,
                hash: p.hash,
                cache_output_dir: p.cache_output_dir,
                roots: p.roots,
            })
            .collect::<Vec<_>>())
    }
}

const LOGS_CACHE_FILE: &str = "logs.cache";
const CACHE_OUTPUT_METADATA_FILE: &str = "cache.meta.bin";

#[async_trait::async_trait]
impl TaskOutputCacheStore for LocalTaskOutputCacheStore {
    type Error = LocalTaskOutputCacheStoreError;

    async fn cache_many(
        &self,
        cache_infos: &[crate::CacheInfo],
    ) -> Result<(), Self::Error> {
        let project_infos =
            cache_infos.iter().map(|i| i.project).collect::<Vec<_>>();
        let results = self
            .collect(
                &project_infos,
                &CollectConfig {
                    hashes: true,
                    cache_output_dirs: true,
                    input_files: true,
                    output_files: true,
                },
            )
            .await?;

        let logs_map = cache_infos
            .iter()
            .map(|r| (r.project.name, r.logs))
            .collect::<Map<_, _>>();

        for result in results {
            let output_dir =
                result.cache_output_dir.as_deref().expect("should be some");
            let output_files = result.output_files.expect("should be some");

            // clear up just in case before writing new files
            if self.sys.fs_exists_async(output_dir).await? {
                self.sys.fs_remove_dir_all_async(output_dir).await?;
            }

            self.sys.fs_create_dir_all_async(output_dir).await?;

            let log_content = logs_map[&result.project.name];
            let logs_path = if log_content.is_some() {
                Some(output_dir.join(LOGS_CACHE_FILE))
            } else {
                None
            };

            if let (Some(logs_path), Some(logs_content)) =
                (logs_path.as_ref(), log_content)
            {
                self.sys.fs_write_async(logs_path, logs_content).await?;
            }

            let mut cache_output_files = vec![];

            for path in output_files {
                let original_abs_path = path.resolve(&result.roots);
                let cache_abs_file_path = output_dir
                    .join(format!("{}.cache", hashtext(&path.to_string())));

                self.sys
                    .fs_hard_link_async(
                        &original_abs_path,
                        &cache_abs_file_path,
                    )
                    .await?;

                cache_output_files.push(CachedFileOutput {
                    cached_path: relpath(&cache_abs_file_path, output_dir)
                        .to_path_buf(),
                    original_path: path,
                });
            }

            // Internally all cached files are relative to the cached output dir
            let metadata = CachedOutput {
                logs_path: logs_path
                    .map(|p| relpath(&p, output_dir).to_path_buf()),
                files: cache_output_files,
            };
            let bytes = bincode::serde::encode_to_vec(
                &metadata,
                bincode::config::standard(),
            )?;

            let metadata_path = output_dir.join(CACHE_OUTPUT_METADATA_FILE);

            self.sys.fs_write_async(&metadata_path, &bytes).await?;
        }

        Ok(())
    }

    async fn get_many(
        &self,
        projects: &[ProjectInfo],
    ) -> Result<Vec<Option<CachedOutput>>, Self::Error> {
        let mut outputs = vec![];

        let config = CollectConfig {
            cache_output_dirs: true,
            ..Default::default()
        };
        'outer_loop: for project in self.collect(projects, &config).await? {
            let output_dir = project.cache_output_dir.expect("should be some");
            let file = output_dir.join(CACHE_OUTPUT_METADATA_FILE);

            let cache_abs = |p: &Path| std::path::absolute(output_dir.join(p));

            let output = if self.sys.fs_exists_async(&file).await? {
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
                        outputs.push(None);
                        continue 'outer_loop;
                    }

                    *logs_path = self.sys.fs_canonicalize_async(p).await?;
                }

                for file in cached_output.files.iter_mut() {
                    let c = cache_abs(&file.cached_path)?;

                    if !self.sys.fs_exists_async(&c).await? {
                        outputs.push(None);
                        continue 'outer_loop;
                    }

                    file.cached_path =
                        self.sys.fs_canonicalize_async(c).await?;
                }

                Some(cached_output)
            } else {
                None
            };

            outputs.push(output);
        }

        Ok(outputs)
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

fn populate_includes_and_globset(
    files: &[OmniPath],
    project_dir: &Path,
    includes: &mut Vec<PathBuf>,
    roots: &RootMap,
    output_files_globset: &mut GlobSetBuilder,
) -> Result<(), <LocalTaskOutputCacheStore as TaskOutputCacheStore>::Error> {
    for p in files {
        let p = p.resolve(roots);

        let path = if p.is_relative() {
            std::path::absolute(project_dir.join(p))?
        } else {
            p.to_path_buf()
        };

        let glob = Glob::new(path.to_string_lossy().as_ref())?;
        output_files_globset.add(glob);
        includes.push(path);
    }
    Ok(())
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
    Globset(#[from] globset::Error),

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
    use super::*;
    use crate::{CacheInfo, cache::impls::LocalTaskOutputCacheStore};
    use derive_new::new;
    use std::path::Path;
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
        input_files: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        env_vars: maps::Map<String, String>,
        input_env_cache_keys: Vec<String>,
    }

    fn project_from_static<'a>(
        project: &'a ProjectInfoStatic,
    ) -> ProjectInfo<'a> {
        ProjectInfo::new(
            &project.name,
            &project.dir,
            &project.output_files,
            &project.input_files,
            &project.input_env_cache_keys,
            &project.env_vars,
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
            input_files: vec![OmniPath::new("src/**/*.txt")],
            output_files: vec![OmniPath::new("dist/**/*.js")],
            dir: project_dir,
            env_vars: env_vars(),
            input_env_cache_keys: env_cache_keys(),
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: None,
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project.get().clone(),
            })
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

    #[tokio::test]
    async fn test_multiple_projects() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project1 = project("project1", dir);
        let project2 = project("project2", dir);

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project1.get().clone(),
            })
            .await
            .expect("failed to cache");

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project2.get().clone(),
            })
            .await
            .expect("failed to cache");

        let cached_output1 = cache
            .get(project1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(project2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_multiple_projects_with_rename_folder() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project1 = project("project1", dir);
        let project2 = project("project2", dir);

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project1.get().clone(),
            })
            .await
            .expect("failed to cache");

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project2.get().clone(),
            })
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(project2.get().dir, dir.join("project2-renamed"))
            .expect("failed to rename");

        let cached_output1 = cache
            .get(project1.get())
            .await
            .expect("failed to get cached output");

        let project2_renamed = project_with_mut("project2", dir, |p| {
            p.dir = dir.join("project2-renamed");
        });

        let cached_output2 = cache
            .get(project2_renamed.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_multiple_projects_with_rename_file() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project1 = project("project1", dir);
        let project2 = project("project2", dir);

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project1.get().clone(),
            })
            .await
            .expect("failed to cache");

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project2.get().clone(),
            })
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(
                project2.get().dir.join("src/a-test.txt"),
                project2.get().dir.join("src/a-test-renamed.txt"),
            )
            .expect("failed to rename");

        let cached_output1 = cache
            .get(project1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(project2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_none(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_multiple_projects_with_modify_content() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let project1 = project("project1", dir);
        let project2 = project("project2", dir);

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project1.get().clone(),
            })
            .await
            .expect("failed to cache");

        cache
            .cache(&CacheInfo {
                logs: Some(LOGS_CONTENT),
                project: project2.get().clone(),
            })
            .await
            .expect("failed to cache");

        // modify the file content
        sys()
            .fs_write_async(
                project2.get().dir.join("src/a-test.txt"),
                "new content",
            )
            .await
            .expect("failed to write file");

        let cached_output1 = cache
            .get(project1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(project2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_none(), "cached output should exist");
    }
}
