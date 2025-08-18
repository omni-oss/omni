use std::path::{Path, PathBuf};

use dir_walker::{
    DirEntry, DirWalker,
    impls::{RealGlobDirWalker, RealGlobDirWalkerConfigBuilderError},
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use maps::{Map, UnorderedMap};
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
};
use omni_types::{OmniPath, Root, RootMap, enum_map};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCanonicalizeAsync as _, FsCreateDirAllAsync, FsHardLinkAsync as _,
    FsMetadataAsync, FsReadAsync as _, FsRemoveDirAllAsync as _,
    FsWriteAsync as _, impls::RealSys,
};
use time::OffsetDateTime;

use crate::{
    CachedFileOutput, CachedTaskExecution, CachedTaskExecutionHash,
    TaskExecutionCacheStore, TaskExecutionInfo,
    hash::{
        ProjectDirHasher,
        impls::{RealDirHasher, RealDirHasherError},
    },
    utils::{has_globs, project_dirname, relpath, remove_globs, topmost_dirs},
};

#[derive(Clone, Debug)]
pub struct LocalTaskExecutionCacheStore {
    sys: RealSys,
    hasher: RealDirHasher,
    cache_dir: PathBuf,
    ws_root_dir: PathBuf,
}

impl LocalTaskExecutionCacheStore {
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
    task: TaskExecutionInfo<'a>,
    input_files: Option<Vec<OmniPath>>,
    output_files: Option<Vec<OmniPath>>,
    hash: Option<DefaultHash>,
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
    pub task_name: &'a str,
    pub task_command: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub input_files: &'a [OmniPath],
    pub input_env_cache_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_hashes: &'a [DefaultHash],
}

fn hashtext(text: &str) -> String {
    let x = DefaultHasher::hash(text.as_bytes());
    bs58::encode(x).into_string()
}

impl LocalTaskExecutionCacheStore {
    fn get_project_dir(&self, project_name: &str) -> PathBuf {
        let name = project_dirname(project_name);

        self.cache_dir.join(name).join("output")
    }

    async fn get_output_dir(
        &self,
        project_name: &str,
        hash: &str,
    ) -> Result<PathBuf, LocalTaskExecutionCacheStoreError> {
        let proj_dir = self.get_project_dir(project_name);
        let output_dir = proj_dir.join(hash);

        Ok(output_dir)
    }

    async fn get_hash(
        &self,
        hash_input: &HashInput<'_>,
    ) -> Result<DefaultHash, LocalTaskExecutionCacheStoreError> {
        let mut tree = self
            .hasher
            .hash_tree::<DefaultHasher>(
                hash_input.project_name,
                hash_input.project_dir,
                hash_input.input_files,
            )
            .await?;

        let mut dep_hashes = hash_input.dependency_hashes.to_vec();

        dep_hashes.sort();

        for dep_hash in dep_hashes {
            tree.insert(dep_hash);
        }

        if !hash_input.env_vars.is_empty() {
            let mut buff = vec![];
            let mut env_keys = hash_input
                .input_env_cache_keys
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>();

            env_keys.sort();

            for env_key in env_keys {
                let value = hash_input
                    .env_vars
                    .get(env_key)
                    .map(|s| s.as_str())
                    .unwrap_or("");

                buff.push(format!("{env_key}={value}"));
            }

            let env_vars = buff.join("\n");

            tree.insert(DefaultHasher::hash(env_vars.as_bytes()));
        }
        let full_task_name = format!(
            "{}#{}: {}",
            hash_input.project_name,
            hash_input.task_name,
            hash_input.task_command
        );
        tree.insert(DefaultHasher::hash(full_task_name.as_bytes()));

        tree.commit();

        Ok(tree.root().expect("unable to get root"))
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "debug", skip(self))
    )]
    async fn collect<'a>(
        &'a self,
        projects: &[TaskExecutionInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, LocalTaskExecutionCacheStoreError> {
        struct Holder<'a> {
            output_files_globset: Option<GlobSet>,
            resolved_output_files: Option<Vec<OmniPath>>,
            input_files_globset: Option<GlobSet>,
            resolved_input_files: Option<Vec<OmniPath>>,
            task: TaskExecutionInfo<'a>,
            roots: RootMap<'a>,
            hash: Option<DefaultHash>,
            cache_output_dir: Option<PathBuf>,
        }

        let mut to_process = Vec::with_capacity(projects.len());

        let should_collect_input_files =
            config.input_files || config.hashes || config.cache_output_dirs;

        let should_collect_output_files = config.output_files;

        // holds paths in input files and output files
        let mut includes = Vec::with_capacity(
            projects
                .iter()
                .map(|i| {
                    let mut count = 0;

                    if should_collect_input_files {
                        count += i.input_files.len();
                    }

                    if should_collect_output_files {
                        count += i.output_files.len();
                    }

                    count
                })
                .sum(),
        );

        for project in projects {
            trace::debug!(
                project_name = ?project.project_name,
                project = ?project,
                "processing project"
            );
            let roots = enum_map! {
                Root::Workspace => &self.ws_root_dir,
                Root::Project => project.project_dir,
            };

            let mut output_files_globset = None;
            if should_collect_output_files {
                let mut output_glob = GlobSetBuilder::new();

                populate_includes_and_globset(
                    project.output_files,
                    project.project_dir,
                    &mut includes,
                    &roots,
                    &mut output_glob,
                )?;

                output_files_globset = Some(output_glob.build()?);
            }

            let mut input_files_globset = None;

            // the output dirs are based on the hashes so need to be collected here
            if should_collect_input_files {
                let mut input_glob = GlobSetBuilder::new();
                populate_includes_and_globset(
                    project.input_files,
                    project.project_dir,
                    &mut includes,
                    &roots,
                    &mut input_glob,
                )?;

                input_files_globset = Some(input_glob.build()?);
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
                task: *project,
                roots,
                hash: None,
                cache_output_dir: None,
            });
        }

        if !includes.is_empty() {
            let topmost =
                topmost_dirs(self.sys.clone(), &includes, &self.ws_root_dir)
                    .into_iter()
                    .map(|p| p.to_path_buf())
                    .collect::<Vec<_>>();

            let topmost =
                topmost.iter().map(|p| p.as_path()).collect::<Vec<_>>();

            // for some reason, ignore doesn't like it when a folder is ignored
            // and a file inside the ignored folder is included
            // so include all the parent folders of the included file
            // so that ignore walks to it and doesn't ignore it
            let mut forced_includes = Vec::with_capacity(includes.len());

            for include in &includes {
                forced_includes.push(include.to_path_buf());

                let clean = if has_globs(include.to_string_lossy().as_ref()) {
                    let clean = remove_globs(include);

                    forced_includes.push(clean.to_path_buf());

                    clean
                } else {
                    include
                };

                for parent in clean.ancestors() {
                    // if we are in the workspace root, stop here
                    if self.ws_root_dir.starts_with(parent) {
                        break;
                    }

                    forced_includes.push(parent.to_path_buf());
                }
            }

            forced_includes.dedup();

            let dirwalker = RealGlobDirWalker::config()
                .standard_filters(true)
                .include(forced_includes)
                .root_dir(&self.ws_root_dir)
                .custom_ignore_filenames(vec![".omniignore".to_string()])
                .build()?
                .build_walker()?;

            for res in dirwalker.walk_dir(&topmost)? {
                let res = res?;
                let original_file_abs_path = res.path();

                if !self.sys.fs_is_file_async(original_file_abs_path).await? {
                    continue;
                }

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
                        task_name: holder.task.task_name,
                        task_command: holder.task.task_command,
                        project_name: holder.task.project_name,
                        project_dir: holder.task.project_dir,
                        input_files: holder
                            .resolved_input_files
                            .as_ref()
                            .expect("should be some"),
                        input_env_cache_keys: holder.task.input_env_keys,
                        env_vars: holder.task.env_vars,
                        dependency_hashes: holder.task.dependency_hashes,
                    })
                    .await?;

                holder.hash = Some(hash);
            }
        }

        if config.cache_output_dirs {
            for holder in &mut to_process {
                let hashstring =
                    bs58::encode(holder.hash.as_ref().expect("should be some"))
                        .into_string();
                let output_dir = self
                    .get_output_dir(holder.task.project_name, &hashstring)
                    .await?;

                holder.cache_output_dir = Some(output_dir);
            }
        }

        Ok(to_process
            .into_iter()
            .map(|p| CollectResult {
                task: p.task,
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
impl TaskExecutionCacheStore for LocalTaskExecutionCacheStore {
    type Error = LocalTaskExecutionCacheStoreError;

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "debug", skip(self))
    )]
    async fn cache_many<'a>(
        &'a self,
        cache_infos: &[crate::NewCacheInfo<'a>],
    ) -> Result<Vec<CachedTaskExecutionHash<'a>>, Self::Error> {
        let task_infos = cache_infos.iter().map(|i| i.task).collect::<Vec<_>>();
        let cache_info_map = cache_infos
            .iter()
            .map(|i| {
                (format!("{}#{}", i.task.project_name, i.task.task_name), *i)
            })
            .collect::<UnorderedMap<_, _>>();

        let results = self
            .collect(
                &task_infos,
                &CollectConfig {
                    hashes: true,
                    cache_output_dirs: true,
                    input_files: true,
                    output_files: true,
                },
            )
            .await?;

        trace::debug!(
            results = ?results,
            "collected results: {}",
            results.len()
        );

        let logs_map = cache_infos
            .iter()
            .map(|r| (r.task.project_name, r.logs))
            .collect::<Map<_, _>>();

        let mut cache_exec_hashes = Vec::with_capacity(results.len());

        for result in results {
            let output_dir =
                result.cache_output_dir.as_deref().expect("should be some");
            let output_files = result.output_files.expect("should be some");

            // clear up just in case before writing new files
            if self.sys.fs_exists_async(output_dir).await? {
                self.sys.fs_remove_dir_all_async(output_dir).await?;
            }

            self.sys.fs_create_dir_all_async(output_dir).await?;

            let log_content = logs_map[&result.task.project_name];
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

            let mut cache_output_files = Vec::with_capacity(output_files.len());

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

                trace::debug!(
                    original_path = ?original_abs_path,
                    cache_path = ?cache_abs_file_path,
                    "hard linked file"
                );

                cache_output_files.push(CachedFileOutput {
                    cached_path: relpath(&cache_abs_file_path, output_dir)
                        .to_path_buf(),
                    original_path: path,
                });
            }

            let tfqn = format!(
                "{}#{}",
                result.task.project_name, result.task.task_name
            );

            let new_cache_info =
                cache_info_map.get(&tfqn).expect("should be some");

            // Internally all cached files are relative to the cached output dir
            let metadata = CachedTaskExecution {
                project_name: result.task.project_name.to_string(),
                logs_path: logs_path
                    .map(|p| relpath(&p, output_dir).to_path_buf()),
                files: cache_output_files,
                task_name: result.task.task_name.to_string(),
                execution_hash: result.hash.expect("should be some"),
                task_command: result.task.task_command.to_string(),
                execution_duration: new_cache_info.execution_duration,
                exit_code: new_cache_info.exit_code,
                execution_time: OffsetDateTime::now_utc(),
            };
            let bytes = bincode::serde::encode_to_vec(
                &metadata,
                bincode::config::standard(),
            )?;

            let metadata_path = output_dir.join(CACHE_OUTPUT_METADATA_FILE);

            self.sys.fs_write_async(&metadata_path, &bytes).await?;

            cache_exec_hashes.push(CachedTaskExecutionHash {
                project_name: result.task.project_name,
                task_name: result.task.task_name,
                execution_hash: result.hash.expect("should be some"),
            });
        }

        Ok(cache_exec_hashes)
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "debug", skip(self))
    )]
    async fn get_many(
        &self,
        projects: &[TaskExecutionInfo],
    ) -> Result<Vec<Option<CachedTaskExecution>>, Self::Error> {
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
                let (mut cached_output, _): (CachedTaskExecution, _) =
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

                    file.original_path.resolve_in_place(&project.roots);
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
    output_globset: &mut GlobSetBuilder,
) -> Result<(), <LocalTaskExecutionCacheStore as TaskExecutionCacheStore>::Error>
{
    for p in files {
        let p = p.resolve(roots);

        let path = if p.is_relative() {
            std::path::absolute(project_dir.join(p))?
        } else {
            p.to_path_buf()
        };

        let glob = Glob::new(path.to_string_lossy().as_ref())?;
        output_globset.add(glob);
        includes.push(path);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct LocalTaskExecutionCacheStoreError {
    kind: LocalTaskExecutionCacheStoreErrorKind,
    #[source]
    inner: LocalTaskExecutionCacheStoreErrorInner,
}

impl LocalTaskExecutionCacheStoreError {
    pub fn kind(&self) -> LocalTaskExecutionCacheStoreErrorKind {
        self.kind
    }
}

impl<T: Into<LocalTaskExecutionCacheStoreErrorInner>> From<T>
    for LocalTaskExecutionCacheStoreError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(
    name(LocalTaskExecutionCacheStoreErrorKind),
    vis(pub),
    repr(u8)
)]
enum LocalTaskExecutionCacheStoreErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    DirHasher(#[from] RealDirHasherError),

    #[error(transparent)]
    Globset(#[from] globset::Error),

    #[error(transparent)]
    RealGlobDirWalkerBuilder(#[from] RealGlobDirWalkerConfigBuilderError),

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
    use crate::{NewCacheInfo, cache::impls::LocalTaskExecutionCacheStore};
    use bytes::Bytes;
    use derive_new::new;
    use std::path::Path;
    use system_traits::{FsRename, FsRenameAsync, impls::RealSys};
    use tokio::io::AsyncReadExt as _;
    use yoke::Yoke;

    const JS_CONTENT: &str = include_str!("../../../test_fixtures/test.js");
    const TXT_CONTENT: &str = include_str!("../../../test_fixtures/test.txt");
    const LOGS_CONTENT: Bytes =
        Bytes::from_static(include_bytes!("../../../test_fixtures/logs.txt"));

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
        // ignore all projects to ensure that input and output files are working
        fixture_with_ignore(projects, "*").await
    }

    async fn __fixture_inner__(projects: &[&str]) -> tempfile::TempDir {
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

    async fn fixture_with_ignore(
        projects: &[&str],
        ignore: &str,
    ) -> tempfile::TempDir {
        let dir = __fixture_inner__(projects).await;
        let root = dir.path();

        let sys = sys();

        sys.fs_create_dir_all_async(root.join(".git"))
            .await
            .expect("failed to create .git dir");

        sys.fs_write_async(root.join(".gitignore"), ignore)
            .await
            .expect("failed to write file");

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
    struct TaskExecutionInfoStatic {
        task_name: String,
        task_command: String,
        project_name: String,
        project_dir: PathBuf,
        input_files: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        env_vars: maps::Map<String, String>,
        input_env_cache_keys: Vec<String>,
    }

    fn task_from_static<'a>(
        task: &'a TaskExecutionInfoStatic,
    ) -> TaskExecutionInfo<'a> {
        TaskExecutionInfo::new(
            task.task_name.as_str(),
            task.task_command.as_str(),
            &task.project_name,
            &task.project_dir,
            &task.output_files,
            &task.input_files,
            &task.input_env_cache_keys,
            &task.env_vars,
            &[],
        )
    }

    fn task_with_mut(
        task_name: &str,
        project_name: &str,
        root_dir: &Path,
        mut f: impl FnMut(&mut TaskExecutionInfoStatic),
    ) -> Yoke<TaskExecutionInfo<'static>, Box<TaskExecutionInfoStatic>> {
        let project_dir = root_dir.join(project_name);
        let mut owned = TaskExecutionInfoStatic {
            task_name: task_name.to_string(),
            task_command: format!("ls {}", task_name),
            project_name: project_name.to_string(),
            input_files: vec![OmniPath::new("src/**/*.txt")],
            output_files: vec![OmniPath::new("dist/**/*.js")],
            project_dir,
            env_vars: env_vars(),
            input_env_cache_keys: env_cache_keys(),
        };
        f(&mut owned);

        Yoke::attach_to_cart(Box::new(owned), |owned| task_from_static(owned))
    }

    #[inline(always)]
    fn task(
        task_name: &str,
        project_name: &str,
        root_dir: &Path,
    ) -> Yoke<TaskExecutionInfo<'static>, Box<TaskExecutionInfoStatic>> {
        task_with_mut(task_name, project_name, root_dir, |_| {})
    }

    fn cache_store(root: &Path) -> LocalTaskExecutionCacheStore {
        LocalTaskExecutionCacheStore::new(root.join(".omni/cache"), root)
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

    fn new_cache_info<'a>(
        logs: Option<&'a Bytes>,
        task: TaskExecutionInfo<'a>,
    ) -> NewCacheInfo<'a> {
        NewCacheInfo {
            logs,
            task,
            exit_code: 0,
            execution_duration: std::time::Duration::from_millis(100),
        }
    }

    #[tokio::test]
    async fn test_cache_unchanged() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);

        let task = task("task", "project1", dir);
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_files_changed_after_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);
        let sys = sys();

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        sys.fs_write_async(
            task.get().project_dir.join("src/a-test.txt"),
            "new content",
        )
        .await
        .expect("failed to write file");

        let cached_output1 = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        // recache then check
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output2 = cache
            .get(task.get())
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
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(None, task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
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
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        // delete a cached file
        let cached_file = &cached_output.files[0];

        tokio::fs::remove_file(&cached_file.cached_path)
            .await
            .expect("failed to delete file");

        let cached_output2 = cache
            .get(task.get())
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
        let mut task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        task = task_with_mut("task", "project1", dir, |project| {
            project
                .env_vars
                .insert("KEY".to_string(), "value-changed".to_string());
        });

        let cached_output1 = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        // recache then check
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output2 = cache
            .get(task.get())
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
        let project = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), project.get().clone()))
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
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
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
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename_async(
                task.get().project_dir,
                dir.join("project1-renamed"),
            )
            .await
            .expect("failed to rename");

        let task_renamed_project_folder =
            task_with_mut("task", "project1", dir, |p| {
                p.project_dir = dir.join("project1-renamed");
            });

        let cached_output = cache
            .get(task_renamed_project_folder.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_renaming_a_file_should_invalidate_cache() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(
                task.get().project_dir.join("src/a-test.txt"),
                task.get().project_dir.join("src/a-test-renamed.txt"),
            )
            .expect("failed to rename");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_use_rooted_omni_path() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task_with_mut("task", "project1", dir, |p| {
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist");

        assert_eq!(cached_output.files.len(), 3, "should be 3 files");
        // the output should be resolved already
        // updated the test to consider that the paths maybe returned in no fixed order
        // due to the parallel nature of the walk
        assert!(
            cached_output.files.iter().any(|f| f
                .original_path
                .path()
                .expect("path should be resolved")
                .ends_with("rootfile.txt")),
            "rootfile.txt must exist in the cached output"
        );
        assert!(
            cached_output.files.iter().any(|f| f
                .original_path
                .path()
                .expect("path should be resolved")
                .ends_with("project1/dist/b-test.js")),
            "project1/dist/b-test.js must exist in the cached output"
        );
        assert!(
            cached_output.files.iter().any(|f| f
                .original_path
                .path()
                .expect("path should be resolved")
                .ends_with("project1/dist/a-test.js")),
            "project1/dist/a-test.js must exist in the cached output"
        );
    }

    #[tokio::test]
    async fn test_multiple_projects() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task1 = task("task", "project1", dir);
        let task2 = task("task", "project2", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task1.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task2.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output1 = cache
            .get(task1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(task2.get())
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
        let task1 = task("task", "project1", dir);
        let task2 = task("task", "project2", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task1.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task2.get().clone()))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(task2.get().project_dir, dir.join("project2-renamed"))
            .expect("failed to rename");

        let cached_output1 = cache
            .get(task1.get())
            .await
            .expect("failed to get cached output");

        let task_project2_renamed =
            task_with_mut("task", "project2", dir, |p| {
                p.project_dir = dir.join("project2-renamed");
            });

        let cached_output2 = cache
            .get(task_project2_renamed.get())
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
        let task1 = task("task", "project1", dir);
        let task2 = task("task", "project2", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task1.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task2.get().clone()))
            .await
            .expect("failed to cache");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(
                task2.get().project_dir.join("src/a-test.txt"),
                task2.get().project_dir.join("src/a-test-renamed.txt"),
            )
            .expect("failed to rename");

        let cached_output1 = cache
            .get(task1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(task2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_multiple_projects_with_modify_content() {
        let temp = fixture(&["project1", "project2"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task1 = task("task", "project1", dir);
        let task2 = task("task", "project2", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task1.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task2.get().clone()))
            .await
            .expect("failed to cache");

        // modify the file content
        sys()
            .fs_write_async(
                task2.get().project_dir.join("src/a-test.txt"),
                "new content",
            )
            .await
            .expect("failed to write file");

        let cached_output1 = cache
            .get(task1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(task2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_invalidate_caches() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .invalidate_caches("project1")
            .await
            .expect("failed to invalidate caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_same_project_different_tasks_should_have_different_hashes() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task1 = task("task1", "project1", dir);
        let task2 = task("task2", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task1.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task2.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output1 = cache
            .get(task1.get())
            .await
            .expect("failed to get cached output");

        let cached_output2 = cache
            .get(task2.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_some(), "cached output should exist");
        assert!(cached_output2.is_some(), "cached output should exist");

        assert_ne!(
            cached_output1.unwrap().execution_hash,
            cached_output2.unwrap().execution_hash,
            "cached output should have different hashes"
        );
    }

    #[tokio::test]
    async fn test_output_files_matching_ignore_files_should_still_be_cached() {
        let temp = fixture_with_ignore(&["project1"], "*").await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task_with_mut("task", "project1", dir, |t| {
            t.output_files
                .push(OmniPath::new_ws_rooted("target/**/*.js"));
        });

        let sys = sys();

        sys.fs_create_dir_all_async(dir.join("target"))
            .await
            .expect("failed to create dir");

        // add file outside of the project that matches the gitignore
        sys.fs_write_async(
            dir.join("target/a-test.js"),
            "console.log('hello')",
        )
        .await
        .expect("failed to write file");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("cached output should exist");

        assert!(cached_output.is_some(), "cached output should exist");
        let output_files = cached_output
            .unwrap()
            .files
            .into_iter()
            .filter(|f| {
                f.original_path.unresolved_path().ends_with("a-test.js")
            })
            .collect::<Vec<_>>();

        assert_eq!(output_files.len(), 2, "should contain the ignored files");
    }
}
