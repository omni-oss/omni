use std::{
    collections::HashSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use dir_walker::impls::RealGlobDirWalkerConfigBuilderError;
use globset::{Glob, GlobSetBuilder};
use maps::{Map, UnorderedMap, unordered_map};
use omni_collector::{CollectConfig, CollectResult, Collector};
use omni_execution_plan::{
    Call, DefaultExecutionPlanProvider, ExecutionPlanProvider,
};
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
};
use omni_remote_cache_client::{
    DefaultRemoteCacheClient, RemoteAccessArgs, RemoteCacheClient,
    RemoteCacheClientError,
};
use omni_task_context::{
    DefaultTaskContextProvider, EnvVars, TaskContextProviderExt,
    TaskHashProvider,
};
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::{
    FsCreateDirAllAsync, FsHardLinkAsync as _, FsMetadataAsync,
    FsReadAsync as _, FsRemoveDirAllAsync as _, FsRenameAsync as _,
    FsWriteAsync as _, impls::RealSys,
};
use time::OffsetDateTime;
use tokio::task::JoinSet;
use trace::Level;

use crate::{
    CacheStats, CacheStatsArgs, CachedFileOutput, CachedTaskExecution,
    CachedTaskExecutionHash, Context, FileCacheStats, ProjectCacheStats,
    PruneCacheArgs, PrunedCacheEntry, StaleStatus, TaskCacheStats,
    TaskExecutionCacheStore, TaskExecutionInfo, TaskExecutionInfoExt,
    impls::{
        cache_archive::archive,
        last_used_db::{LocalLastUsedDb, LocalLastUsedDbError},
        lock::{
            CacheLockGuard, LAST_USED_LOCK_FILE, PRUNE_LOCK_FILE,
            lock_file_path,
        },
    },
};

pub use omni_utils::path::{
    has_globs, path_safe, relpath, remove_globs, topmost_dirs,
};

use omni_hasher::project_dir_hasher::impls::RealDirHasherError;

use super::cache_archive::unarchive;

#[derive(Clone, Debug)]
pub struct HybridTaskExecutionCacheStore {
    sys: RealSys,
    cache_dir: PathBuf,
    ws_root_dir: PathBuf,
    remote_config: RemoteConfig,
    client: Arc<DefaultRemoteCacheClient>,
}

#[derive(
    Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default, new, EnumIs,
)]
pub enum RemoteConfig {
    #[default]
    Disabled,
    Enabled(EnabledRemoteConfig),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default, new)]
pub struct EnabledRemoteConfig {
    #[new(into)]
    pub api_base_url: String,

    #[new(into)]
    pub api_key: String,

    #[new(into)]
    pub tenant_code: String,

    #[new(into)]
    pub organization_code: String,

    #[new(into)]
    pub workspace_code: String,

    #[new(into)]
    pub environment_code: Option<String>,
}

impl HybridTaskExecutionCacheStore {
    pub fn new(
        cache_dir: impl Into<PathBuf>,
        ws_root_dir: impl Into<PathBuf>,
        remote_config: impl Into<RemoteConfig>,
    ) -> Self {
        let dir = cache_dir.into();
        let ws_root_dir = ws_root_dir.into();
        Self {
            sys: RealSys,
            cache_dir: dir,
            ws_root_dir,
            remote_config: remote_config.into(),
            client: Arc::new(DefaultRemoteCacheClient::default()),
        }
    }
}

fn hashtext(text: &str) -> String {
    let x = DefaultHasher::hash(text.as_bytes());
    bs58::encode(x).into_string()
}

impl HybridTaskExecutionCacheStore {
    /// Build the execution plan with the given project/task/dir/meta filters
    /// and return the set of `project#task` task identifiers that survive all
    /// of them. Used to apply the context-dependent `--dir`/`--meta` filters
    /// to cache entries, which are otherwise only keyed by name.
    fn plan_task_set<TContext: Context>(
        &self,
        context: &TContext,
        project_name_globs: &[&str],
        task_name_globs: &[&str],
        dir_globs: &[&str],
        meta_filter: Option<&str>,
    ) -> Result<HashSet<String>, LocalTaskExecutionCacheStoreError> {
        let call = Call::new_tasks(if task_name_globs.is_empty() {
            vec!["*".to_string()]
        } else {
            task_name_globs.iter().map(|s| s.to_string()).collect()
        });

        let plan =
            DefaultExecutionPlanProvider::new(ContextWrapper::new(context))
                .get_execution_plan(
                    &call,
                    project_name_globs,
                    dir_globs,
                    meta_filter,
                    None,
                    false,
                    false,
                )?;

        let mut set =
            HashSet::with_capacity(plan.iter().map(|b| b.len()).sum());
        for batch in plan {
            for node in batch {
                set.insert(format!(
                    "{}#{}",
                    node.project_name(),
                    node.task_name()
                ));
            }
        }

        Ok(set)
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip(self, tasks, config))
    )]
    async fn collect<'a>(
        &'a self,
        tasks: &[TaskExecutionInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, LocalTaskExecutionCacheStoreError> {
        let collect_task_infos = tasks
            .iter()
            .map(|task| omni_collector::ProjectTaskInfo {
                input_files: task.input_files,
                output_files: task.output_files,
                project_dir: task.project_dir,
                project_name: task.project_name,
                task_exec: task.task_exec,
                task_retry_exec: task.task_retry_exec,
                task_name: task.task_name,
                dependency_digests: task.dependency_digests,
                env_vars: task.env_vars,
                input_env_keys: task.input_env_keys,
                args: task.args,
            })
            .collect::<Vec<_>>();

        let to_process = Collector::new(
            self.ws_root_dir.as_path(),
            self.cache_dir.as_path(),
            self.sys.clone(),
        )
        .collect(&collect_task_infos, config)
        .await?;

        Ok(to_process)
    }

    async fn update_last_used_timestamps(
        &self,
        tasks: &[Option<CachedTaskExecution>],
    ) -> Result<(), LocalTaskExecutionCacheStoreError> {
        // Nothing to record: avoid taking the lock or touching the db file.
        if tasks.iter().all(|t| t.is_none()) {
            return Ok(());
        }

        // The timestamps db is a single shared file mutated read-modify-write
        // by every process, so serialize access with an exclusive lock to
        // avoid lost updates and torn writes.
        let _guard = CacheLockGuard::acquire_exclusive(lock_file_path(
            &self.cache_dir,
            LAST_USED_LOCK_FILE,
        ))
        .await?;

        let path = self.cache_dir.join(LAST_USED_TIMESTAMPS_DB_FILE);
        let mut last_used_db = LocalLastUsedDb::load(&path).await?;
        let dir = OffsetDateTime::now_utc();

        for exec in tasks.iter().filter_map(|t| t.as_ref()) {
            last_used_db
                .update_last_used_timestamp(
                    &exec.project_name,
                    &exec.task_name,
                    exec.digest,
                    dir,
                )
                .await?;
        }

        last_used_db.save().await?;
        Ok(())
    }

    /// Atomically publishes a fully-populated staging directory to its final
    /// content-addressed location.
    ///
    /// Because entries are content-addressed, an entry that already exists is
    /// equivalent to ours, so a publish that loses the race is still a
    /// success. Returns once the destination holds a complete entry.
    async fn publish_staging(
        &self,
        staging_dir: &Path,
        output_dir: &Path,
        metadata_path: &Path,
    ) -> Result<(), LocalTaskExecutionCacheStoreError> {
        const MAX_ATTEMPTS: usize = 5;

        for _ in 0..MAX_ATTEMPTS {
            match self.sys.fs_rename_async(staging_dir, output_dir).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // A peer published an identical entry first. Discard our
                    // staging copy and treat this as success.
                    if self.sys.fs_exists_async(metadata_path).await? {
                        let _ =
                            self.sys.fs_remove_dir_all_async(staging_dir).await;
                        return Ok(());
                    }

                    // A stale/partial directory (no metadata sentinel) is
                    // occupying the destination, e.g. left by an older omni
                    // version or a crashed run. Clear it and retry.
                    if self.sys.fs_exists_async(output_dir).await? {
                        let _ =
                            self.sys.fs_remove_dir_all_async(output_dir).await;
                        continue;
                    }

                    // Genuine, unrecoverable failure.
                    let _ = self.sys.fs_remove_dir_all_async(staging_dir).await;
                    return Err(e.into());
                }
            }
        }

        let _ = self.sys.fs_remove_dir_all_async(staging_dir).await;
        Err(std::io::Error::other(
            "failed to publish cache entry after multiple attempts",
        )
        .into())
    }

    /// Removes the given cache entries from disk without taking the prune
    /// lock. Callers must already hold the exclusive prune lock.
    async fn force_prune_inner(
        &self,
        entries: &[PrunedCacheEntry],
    ) -> Result<(), LocalTaskExecutionCacheStoreError> {
        for entry in entries {
            if !tokio::fs::try_exists(&entry.entry_dir).await? {
                log::debug!(
                    "Cache entry does not exist: {}",
                    entry.entry_dir.display()
                );
                continue;
            }

            tokio::fs::remove_dir_all(&entry.entry_dir)
                .await
                .inspect_err(|e| {
                    log::error!("Failed to delete cache entry: {}", e)
                })?;

            log::debug!("Pruned cache entry: {}", entry.entry_dir.display());
        }

        Ok(())
    }

    /// Reclaims staging directories left behind by crashed processes.
    ///
    /// Only directories older than `max_age` are removed, so a
    /// concurrently-publishing peer's fresh staging directory is never
    /// touched. Best-effort: individual failures are logged and skipped.
    async fn prune_stale_staging_dirs(&self, max_age: std::time::Duration) {
        let now = SystemTime::now();
        let mut projects = match tokio::fs::read_dir(&self.cache_dir).await {
            Ok(entries) => entries,
            Err(_) => return,
        };

        while let Ok(Some(project)) = projects.next_entry().await {
            let output_dir = project.path().join("output");
            let mut entries = match tokio::fs::read_dir(&output_dir).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                if !name.to_string_lossy().starts_with(STAGING_PREFIX) {
                    continue;
                }

                let stale = entry
                    .metadata()
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age >= max_age);

                if stale
                    && let Err(e) =
                        tokio::fs::remove_dir_all(entry.path()).await
                {
                    log::debug!(
                        "failed to reclaim stale staging dir {}: {e}",
                        entry.path().display()
                    );
                }
            }
        }
    }
}

const LOGS_CACHE_FILE: &str = "logs.cache";
const CACHE_OUTPUT_METADATA_FILE: &str = "cache.meta.bin";
const LAST_USED_TIMESTAMPS_DB_FILE: &str = "last-used-timestamps.db";

/// Prefix for in-progress staging directories created while publishing a
/// cache entry. A staging directory is populated in full and then atomically
/// renamed into its final (content-addressed) location, so readers never
/// observe a half-written entry. Anything bearing this prefix is internal
/// scratch state and must be ignored by readers/stats and reclaimed by prune.
const STAGING_PREFIX: &str = ".staging-";

/// Staging directories older than this are considered orphaned (left behind by
/// a crashed process) and are reclaimed during prune.
const STALE_STAGING_AGE: std::time::Duration =
    std::time::Duration::from_secs(60 * 60);

/// Process-wide counter ensuring staging directory names are unique even when
/// a single process publishes the same digest from multiple tasks at once.
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Builds a unique staging directory path that is a sibling of `output_dir`
/// (and therefore on the same filesystem, so the publishing rename is atomic).
fn staging_dir_for(output_dir: &Path) -> PathBuf {
    let parent = output_dir.parent().unwrap_or(output_dir);
    let digest_name = output_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);

    parent.join(format!("{STAGING_PREFIX}{digest_name}-{pid}-{nanos}-{seq}"))
}

/// Returns whether a directory entry name denotes internal cache scratch state
/// (staging dirs or the lock folder) rather than a real cache entry.
fn is_internal_cache_entry(name: &str) -> bool {
    name.starts_with('.')
}

#[async_trait::async_trait]
impl TaskExecutionCacheStore for HybridTaskExecutionCacheStore {
    type Error = LocalTaskExecutionCacheStoreError;

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(level = Level::DEBUG, skip_all))]
    async fn cache_many<'a>(
        &'a self,
        cache_infos: &[crate::NewCacheInfo<'a>],
    ) -> Result<Vec<CachedTaskExecutionHash<'a>>, Self::Error> {
        trace::trace!(?cache_infos, "begin_cache_many");

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
                    digests: true,
                    cache_output_dirs: true,
                    input_files: true,
                    output_files: true,
                },
            )
            .await?;

        trace::trace!(
            results_count = results.len(),
            "collected_results_for_caching"
        );

        let logs_map = cache_infos
            .iter()
            .map(|r| (r.task.project_name, r.logs))
            .collect::<Map<_, _>>();

        let mut cache_exec_hashes = Vec::with_capacity(results.len());

        let mut cached_results = if self.remote_config.is_enabled() {
            Vec::with_capacity(results.len())
        } else {
            Vec::new()
        };

        // Hold a shared prune lock for the duration of publishing so a
        // concurrent prune cannot delete an entry we are renaming into place.
        // Multiple publishers share the lock and never block each other.
        let _prune_guard = CacheLockGuard::acquire_shared(lock_file_path(
            &self.cache_dir,
            PRUNE_LOCK_FILE,
        ))
        .await?;

        for result in results {
            let output_dir =
                result.cache_output_dir.as_deref().expect("should be some");
            let output_files =
                result.output_files.as_ref().expect("should be some");

            let metadata_path = output_dir.join(CACHE_OUTPUT_METADATA_FILE);
            let log_content = logs_map[&result.task.project_name];
            let digest = result.digest.expect("should be some");

            // Cache entries are content-addressed by their digest, so an
            // existing complete entry is byte-for-byte equivalent to what we
            // would write. Reuse it instead of rewriting (which would race
            // with peers reading or publishing the same digest).
            let already_published =
                self.sys.fs_exists_async(&metadata_path).await?;

            if !already_published {
                let tfqn = format!(
                    "{}#{}",
                    result.task.project_name, result.task.task_name
                );
                let new_cache_info =
                    cache_info_map.get(&tfqn).expect("should be some");

                // Build the entry in an isolated staging directory and then
                // atomically rename it into place, so readers only ever see a
                // fully-formed entry.
                let staging_dir = staging_dir_for(output_dir);

                let build = async {
                    self.sys.fs_create_dir_all_async(&staging_dir).await?;

                    let logs_rel = if let Some(logs_content) = log_content {
                        self.sys
                            .fs_write_async(
                                staging_dir.join(LOGS_CACHE_FILE),
                                logs_content,
                            )
                            .await?;
                        Some(PathBuf::from(LOGS_CACHE_FILE))
                    } else {
                        None
                    };

                    let mut cache_output_files =
                        Vec::with_capacity(output_files.len());

                    for path in output_files.iter() {
                        let original_abs_path = path.resolve(&result.roots);
                        let cache_file_name = format!(
                            "{}.cache",
                            hashtext(&path.to_string())
                        );
                        let cache_abs_file_path =
                            staging_dir.join(&cache_file_name);

                        self.sys
                            .fs_hard_link_async(
                                &original_abs_path,
                                &cache_abs_file_path,
                            )
                            .await?;

                        log::debug!(
                            "cache file hard link {original_abs_path:?} to {cache_abs_file_path:?}"
                        );

                        cache_output_files.push(CachedFileOutput {
                            cached_path: PathBuf::from(cache_file_name),
                            original_path: path.clone(),
                        });
                    }

                    // All cached paths are stored relative to the entry dir.
                    let metadata = CachedTaskExecution {
                        project_name: result.task.project_name.to_string(),
                        logs_path: logs_rel,
                        files: cache_output_files,
                        task_name: result.task.task_name.to_string(),
                        digest,
                        task_exec: result.task.task_exec.map(|c| c.to_string()),
                        task_retry_exec: result
                            .task
                            .task_retry_exec
                            .map(|c| c.to_string()),
                        execution_duration: new_cache_info.execution_duration,
                        exit_code: new_cache_info.exit_code,
                        execution_time: OffsetDateTime::now_utc(),
                        dependency_digests: new_cache_info
                            .task
                            .dependency_digests
                            .to_vec(),
                        tries: new_cache_info.tries,
                    };
                    let bytes = rmp_serde::encode::to_vec(&metadata)?;

                    // Write the metadata last: its presence is the sentinel
                    // that marks the entry complete.
                    self.sys
                        .fs_write_async(
                            staging_dir.join(CACHE_OUTPUT_METADATA_FILE),
                            &bytes,
                        )
                        .await?;

                    Ok::<(), LocalTaskExecutionCacheStoreError>(())
                }
                .await;

                if let Err(e) = build {
                    let _ =
                        self.sys.fs_remove_dir_all_async(&staging_dir).await;
                    return Err(e);
                }

                self.publish_staging(&staging_dir, output_dir, &metadata_path)
                    .await?;
            }

            let execution_hash = CachedTaskExecutionHash {
                project_name: result.task.project_name,
                task_name: result.task.task_name,
                digest: result.digest.expect("should be some"),
            };

            cache_exec_hashes.push(execution_hash);

            if self.remote_config.is_enabled() {
                cached_results.push((execution_hash, output_dir.to_path_buf()));
            }
        }

        if !cached_results.is_empty()
            && let RemoteConfig::Enabled(conf) = &self.remote_config
        {
            log::debug!("Uploading remote cache artifacts...");
            let mut tasks = JoinSet::new();
            for (hash, output_dir) in cached_results {
                let client = self.client.clone();
                let conf = conf.clone();
                let digest = bs58::encode(hash.digest).into_string();

                tasks.spawn(async move {
                    let config = RemoteAccessArgs {
                        api_key: &conf.api_key,
                        api_base_url: &conf.api_base_url,
                        env: conf
                            .environment_code
                            .as_deref()
                            .unwrap_or("default"),
                        org: &conf.organization_code,
                        tenant: &conf.tenant_code,
                        ws: &conf.workspace_code,
                    };

                    if client.artifact_exists(&config, &digest).await? {
                        return Ok::<_, LocalTaskExecutionCacheStoreError>(());
                    }

                    let mut artifact = Vec::new();
                    archive(&output_dir, Cursor::new(&mut artifact))?;

                    client
                        .put_artifact(&config, &digest, Bytes::from(artifact))
                        .await?;

                    log::debug!("Uploaded cache for {}", digest);

                    Ok::<_, LocalTaskExecutionCacheStoreError>(())
                });
            }

            let results = tasks.join_all().await;

            for result in results {
                result?;
            }
        }

        Ok(cache_exec_hashes)
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip(self, projects))
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

        let collected = self.collect(projects, &config).await?;

        if let RemoteConfig::Enabled(conf) = &self.remote_config {
            let mut tasks = JoinSet::new();
            for project in &collected {
                let output_dir = project
                    .cache_output_dir
                    .as_deref()
                    .expect("should be some");

                // skip if the output dir already exists/downloaded
                if tokio::fs::try_exists(output_dir).await? {
                    continue;
                }

                let digest =
                    bs58::encode(project.digest.unwrap()).into_string();

                let client = self.client.clone();
                let conf = conf.clone();
                let output_dir = output_dir.to_path_buf();

                tasks.spawn(async move {
                    let response = client
                        .get_artifact(
                            &RemoteAccessArgs {
                                api_key: &conf.api_key,
                                api_base_url: &conf.api_base_url,
                                env: conf
                                    .environment_code
                                    .as_deref()
                                    .unwrap_or("default"),
                                org: &conf.organization_code,
                                tenant: &conf.tenant_code,
                                ws: &conf.workspace_code,
                            },
                            &digest,
                        )
                        .await?;

                    if let Some(bytes) = response {
                        log::debug!("fetched remote cache for {}", digest);
                        unarchive(&output_dir, Cursor::new(bytes))?;
                    }

                    Ok::<_, LocalTaskExecutionCacheStoreError>(())
                });
            }

            let results = tasks.join_all().await;

            for result in results {
                result?;
            }
        }

        'outer_loop: for project in &collected {
            let output_dir =
                project.cache_output_dir.as_deref().expect("should be some");
            let file = output_dir.join(CACHE_OUTPUT_METADATA_FILE);

            let cache_abs = |p: &Path| std::path::absolute(output_dir.join(p));

            let output = if self.sys.fs_exists_async(&file).await? {
                let bytes = self.sys.fs_read_async(&file).await?;
                let mut cached_output: CachedTaskExecution =
                    rmp_serde::decode::from_slice(&bytes)?;

                // canonicalize the paths
                if let Some(logs_path) = cached_output.logs_path.as_mut() {
                    let p = cache_abs(logs_path)?;

                    if !self.sys.fs_exists_async(&p).await? {
                        outputs.push(None);
                        continue 'outer_loop;
                    }

                    *logs_path = p;
                }

                for file in cached_output.files.iter_mut() {
                    let c = cache_abs(&file.cached_path)?;

                    if !self.sys.fs_exists_async(&c).await? {
                        outputs.push(None);
                        continue 'outer_loop;
                    }

                    file.cached_path = c;

                    file.original_path.resolve_in_place(&project.roots);
                }

                Some(cached_output)
            } else {
                None
            };

            outputs.push(output);
        }

        self.update_last_used_timestamps(&outputs).await?;

        Ok(outputs)
    }

    async fn get_stats<TContext: Context>(
        &self,
        args: &CacheStatsArgs<'_, TContext>,
    ) -> Result<CacheStats, Self::Error> {
        let project_name_globs = args.project_name_globs;
        let task_name_globs = args.task_name_globs;

        let mut entries = tokio::fs::read_dir(&self.cache_dir).await?;

        let project_glob = {
            let project_name_globs = if project_name_globs.is_empty() {
                &["*"]
            } else {
                project_name_globs
            };
            let mut glob_set = GlobSetBuilder::new();
            for project_name_glob in project_name_globs {
                glob_set.add(Glob::new(project_name_glob)?);
            }
            glob_set.build()?
        };
        let task_glob = {
            let task_name_globs = if task_name_globs.is_empty() {
                &["*"]
            } else {
                task_name_globs
            };
            let mut glob_set = GlobSetBuilder::new();
            for task_name_glob in task_name_globs {
                glob_set.add(Glob::new(task_name_glob)?);
            }
            glob_set.build()?
        };

        let mut futs = JoinSet::new();

        let last_used_db_path =
            self.cache_dir.join(LAST_USED_TIMESTAMPS_DB_FILE);

        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }

            let file_name = entry.file_name();
            let enc_project_name = file_name.to_string_lossy();

            // Skip internal scratch state (the `.locks` folder, staging dirs,
            // etc.); it is never a bs58-encoded project directory.
            if is_internal_cache_entry(&enc_project_name) {
                continue;
            }

            let project_name =
                bs58::decode(&enc_project_name as &str).into_vec()?;
            let project_name =
                String::from_utf8_lossy(&project_name).to_string();

            if !project_glob.is_match(&project_name) {
                continue;
            }

            let t_task_glob = task_glob.clone();
            futs.spawn(async move {
                let mut tasks = vec![];
                let project_output_dir = &entry.path().join("output");

                if !tokio::fs::try_exists(project_output_dir).await? {
                    log::debug!(
                        "no output dir found for {}",
                        entry.path().display()
                    );

                    return Ok::<_, LocalTaskExecutionCacheStoreError>(
                        ProjectCacheStats {
                            project_name: project_name.to_string(),
                            tasks,
                        },
                    );
                }

                let mut task_entries =
                    tokio::fs::read_dir(project_output_dir).await?;

                while let Some(entry) = task_entries.next_entry().await? {
                    if !entry.file_type().await?.is_dir() {
                        log::debug!(
                            "not a directory: {}",
                            entry.path().display()
                        );
                        continue;
                    }

                    // Skip in-progress staging directories; they are not yet
                    // published cache entries.
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(STAGING_PREFIX)
                    {
                        continue;
                    }

                    let cache_meta_path =
                        entry.path().join(CACHE_OUTPUT_METADATA_FILE);

                    if !tokio::fs::try_exists(&cache_meta_path).await? {
                        log::debug!(
                            "no cache metadata found for {}",
                            entry.path().display()
                        );
                        continue;
                    }

                    let cache_meta_bytes =
                        tokio::fs::read(&cache_meta_path).await?;

                    let cache_meta: CachedTaskExecution =
                        rmp_serde::decode::from_slice(&cache_meta_bytes)?;

                    if !t_task_glob.is_match(&cache_meta.task_name) {
                        continue;
                    }

                    let meta_file = load_stats(&cache_meta_path);

                    let log_file = async {
                        if let Some(logs_path) = cache_meta.logs_path {
                            Ok(Some(
                                load_stats(entry.path().join(logs_path))
                                    .await?,
                            ))
                        } else {
                            Ok(None)
                        }
                    };

                    let mut files_set = JoinSet::new();

                    for file in cache_meta.files {
                        files_set.spawn(load_stats(
                            entry.path().join(file.cached_path),
                        ));
                    }

                    let (meta_file, log_file) =
                        tokio::try_join!(meta_file, log_file)?;

                    let output = files_set.join_all().await;
                    let mut cached_files = vec![];

                    for file in output {
                        cached_files.push(file?);
                    }
                    let cached_files_total_size = cached_files
                        .iter()
                        .map(|f| f.size.as_u64())
                        .sum::<u64>();
                    let total_size = cached_files_total_size
                        + meta_file.size.as_u64()
                        + log_file
                            .as_ref()
                            .map(|f| f.size.as_u64())
                            .unwrap_or(0);

                    tasks.push(TaskCacheStats {
                        task_name: cache_meta.task_name.to_string(),
                        digest: cache_meta.digest,
                        created_timestamp: cache_meta.execution_time,
                        execution_duration: cache_meta.execution_duration,
                        total_size: ByteSize::b(total_size),
                        cached_files_total_size: ByteSize::b(
                            cached_files_total_size,
                        ),
                        cached_files,
                        last_used_timestamp: None,
                        meta_file,
                        log_file,
                        entry_dir: entry.path().to_path_buf(),
                    });
                }

                //
                Ok::<_, LocalTaskExecutionCacheStoreError>(ProjectCacheStats {
                    project_name: project_name.to_string(),
                    tasks,
                })
            });
        }

        let results = futs.join_all().await;
        let mut projects = vec![];
        let last_used_db = LocalLastUsedDb::load(&last_used_db_path).await?;
        for result in results {
            let mut result = result?;

            for task in result.tasks.iter_mut() {
                task.last_used_timestamp = last_used_db
                    .get_last_used_timestamp(
                        &result.project_name,
                        &task.task_name,
                        task.digest,
                    )
                    .await?;
            }

            projects.push(result);
        }

        let mut stats = CacheStats { projects };

        // `--dir`/`--meta` are context-dependent: matching a cached entry to a
        // directory or meta config requires the current workspace. Build the
        // execution plan (which applies project/task/dir/meta filtering) and
        // keep only the entries whose task survives those filters. Entries for
        // tasks/projects that no longer exist cannot be matched and are
        // dropped from the filtered report.
        if !args.dir_globs.is_empty() || args.meta_filter.is_some() {
            let context = args.context.as_ref().ok_or_else(|| {
                LocalTaskExecutionCacheStoreError::from(
                    LocalTaskExecutionCacheStoreErrorInner::MissingContext,
                )
            })?;

            let allowed = self.plan_task_set(
                context,
                args.project_name_globs,
                args.task_name_globs,
                args.dir_globs,
                args.meta_filter,
            )?;

            for project in stats.projects.iter_mut() {
                let project_name = project.project_name.clone();
                project.tasks.retain(|task| {
                    allowed.contains(&format!(
                        "{}#{}",
                        project_name, task.task_name
                    ))
                });
            }
            stats.projects.retain(|project| !project.tasks.is_empty());
        }

        Ok(stats)
    }

    async fn prune_caches<TContext: Context>(
        &self,
        args: &PruneCacheArgs<'_, TContext>,
    ) -> Result<Vec<PrunedCacheEntry>, Self::Error> {
        // Take the exclusive prune lock for the whole operation so we never
        // delete an entry a peer is publishing into. Publishers hold this lock
        // in shared mode, so they are excluded while we prune.
        let _prune_guard = CacheLockGuard::acquire_exclusive(lock_file_path(
            &self.cache_dir,
            PRUNE_LOCK_FILE,
        ))
        .await?;

        let stats = self
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: args.project_name_globs,
                task_name_globs: args.task_name_globs,
                ..Default::default()
            })
            .await?;
        let time_upper_limit = if let Some(older_than) = args.older_than {
            Some(OffsetDateTime::now_utc() - older_than)
        } else {
            None
        };

        let mut entries = vec![];

        // `--dir` and `--meta` are "context-dependent" filters: matching a
        // cached entry requires resolving the owning project's directory and
        // meta configuration from the *current* workspace, which only the
        // loaded context knows. `--stale-only` likewise needs the context to
        // recompute fresh digests. Whenever any of these is requested we build
        // a single execution plan (which already applies project/task/dir/meta
        // filtering) and derive from it:
        //   * `allowed_tasks` - the set of `project#task` that survive the
        //     dir/meta filters, used to gate which entries may be pruned, and
        //   * `hashes` - the current fresh digest per task, used by
        //     `--stale-only` to keep entries that are still fresh.
        let needs_dir_or_meta_filter =
            !args.dir_globs.is_empty() || args.meta_filter.is_some();
        let needs_plan = args.stale_only || needs_dir_or_meta_filter;

        let (allowed_tasks, hashes) = if needs_plan {
            let context = args.context.as_ref().ok_or_else(|| {
                LocalTaskExecutionCacheStoreError::from(
                    LocalTaskExecutionCacheStoreErrorInner::MissingContext,
                )
            })?;

            let time_now = SystemTime::now();

            let call = Call::new_tasks(if args.task_name_globs.is_empty() {
                vec!["*".to_string()]
            } else {
                args.task_name_globs.iter().map(|s| s.to_string()).collect()
            });
            let plan =
                DefaultExecutionPlanProvider::new(ContextWrapper::new(context))
                    .get_execution_plan(
                        &call,
                        args.project_name_globs,
                        args.dir_globs,
                        args.meta_filter,
                        None,
                        false,
                        false,
                    )?;

            let total_task = plan.iter().map(|b| b.len()).sum();

            let mut allowed_tasks = if needs_dir_or_meta_filter {
                Some(HashSet::with_capacity(total_task))
            } else {
                None
            };

            let mut hashes = unordered_map!(cap: total_task);
            let collect_cfg = CollectConfig {
                digests: true,
                ..Default::default()
            };

            for batch in plan {
                if let Some(allowed) = &mut allowed_tasks {
                    for node in &batch {
                        allowed.insert(format!(
                            "{}#{}",
                            node.project_name(),
                            node.task_name()
                        ));
                    }
                }

                // Fresh digests are only needed for `--stale-only`; skip the
                // (potentially expensive) collection otherwise.
                if !args.stale_only {
                    continue;
                }

                let hash_provider = HashProvider::new(&hashes);
                let task_ctx_provider = DefaultTaskContextProvider::new(
                    hash_provider,
                    ContextWrapper::new(context),
                );
                let contexts =
                    task_ctx_provider.get_task_contexts(&batch, false)?;

                let exec_infos = contexts
                    .iter()
                    .flat_map(|c| c.execution_info())
                    .collect::<Vec<_>>();

                let results = self.collect(&exec_infos, &collect_cfg).await?;
                let mut t_hashes = unordered_map!(cap: batch.len());

                for result in results {
                    let hash = result.digest.expect("should be some");

                    let task_full_name = format!(
                        "{}#{}",
                        result.task.project_name, result.task.task_name
                    );
                    t_hashes.insert(task_full_name, hash);
                }

                hashes.extend(t_hashes);
            }

            let hashes = if args.stale_only {
                log::debug!(
                    "getting hashes for stale elapsed time: {:?}",
                    time_now.elapsed().unwrap()
                );
                Some(hashes)
            } else {
                None
            };

            (allowed_tasks, hashes)
        } else {
            (None, None)
        };

        for project in stats.projects {
            for task in project.tasks {
                if let Some(time_upper_limit) = time_upper_limit {
                    let last_used = task
                        .last_used_timestamp
                        .unwrap_or(task.created_timestamp);
                    if last_used >= time_upper_limit {
                        continue;
                    }
                }

                if let Some(larger_than) = args.larger_than
                    && task.total_size <= larger_than
                {
                    continue;
                }

                let task_full_name =
                    format!("{}#{}", project.project_name, task.task_name);

                // When `--dir`/`--meta` are active, only prune entries whose
                // task survived those filters in the current workspace. Entries
                // for tasks/projects that no longer exist (orphaned) cannot be
                // matched by dir/meta and are therefore left untouched.
                if let Some(allowed) = &allowed_tasks
                    && !allowed.contains(&task_full_name)
                {
                    continue;
                }

                if args.stale_only
                    && let Some(hashes) = &hashes
                    && let Some(hash) = hashes.get(&task_full_name)
                    && task.digest == *hash
                {
                    continue;
                }

                entries.push(PrunedCacheEntry {
                    project_name: project.project_name.clone(),
                    task_name: task.task_name,
                    digest: task.digest,
                    size: task.total_size,
                    entry_dir: task.entry_dir,
                    stale: StaleStatus::Unknown,
                });
            }
        }

        if !args.dry_run {
            // Already holding the exclusive prune lock; use the inner helper to
            // avoid re-acquiring it (which would deadlock).
            self.force_prune_inner(&entries).await?;
            self.prune_stale_staging_dirs(STALE_STAGING_AGE).await;
        }

        Ok(entries)
    }

    async fn force_prune_caches(
        &self,
        entries: &[PrunedCacheEntry],
    ) -> Result<(), Self::Error> {
        let _prune_guard = CacheLockGuard::acquire_exclusive(lock_file_path(
            &self.cache_dir,
            PRUNE_LOCK_FILE,
        ))
        .await?;

        self.force_prune_inner(entries).await
    }
}

#[derive(new, Clone, Copy)]
struct HashProvider<'a> {
    hashes: &'a UnorderedMap<String, DefaultHash>,
}

impl<'a> TaskHashProvider for HashProvider<'a> {
    fn get_task_hash(&self, task_full_name: &str) -> Option<DefaultHash> {
        self.hashes.get(task_full_name).cloned()
    }
}

#[derive(new, Clone, Copy)]
struct ContextWrapper<'a, T: Context + 'a> {
    inner: &'a T,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, T: Context> omni_execution_plan::Context for ContextWrapper<'a, T> {
    type Error = T::Error;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_project_meta_config(project_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_task_meta_config(project_name, task_name)
    }

    fn get_project_graph(
        &self,
    ) -> Result<omni_core::ProjectGraph, Self::Error> {
        self.inner.get_project_graph()
    }

    fn projects(&self) -> &[omni_core::Project] {
        self.inner.projects()
    }

    fn get_cache_input_files(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> &[OmniPath] {
        self.inner
            .get_cache_info(project_name, task_name)
            .map(|c| &c.key_input_files[..])
            .unwrap_or(&[])
    }

    fn root_dir(&self) -> &Path {
        self.inner.root_dir()
    }
}

impl<'a, T: Context> omni_task_context::Context for ContextWrapper<'a, T> {
    type Error = T::Error;

    fn get_task_env_vars(
        &self,
        node: &omni_core::TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVars>>, Self::Error> {
        self.inner.get_task_env_vars(node)
    }

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_context::CacheInfo> {
        self.inner.get_cache_info(project_name, task_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_task_meta_config(task_name, project_name)
    }

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.inner.get_project_meta_config(project_name)
    }

    fn get_task_override_args(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>> {
        self.inner.get_task_override_args(project_name, task_name)
    }
}

async fn load_stats<P: AsRef<Path> + Clone>(
    path: P,
) -> Result<FileCacheStats, LocalTaskExecutionCacheStoreError> {
    let path_ref = path.as_ref();
    let meta = tokio::fs::metadata(path_ref).await.inspect_err(|e| {
        log::error!("failed to get metadata for {}: {}", path_ref.display(), e)
    })?;

    Ok::<_, LocalTaskExecutionCacheStoreError>(FileCacheStats {
        path: path_ref.to_string_lossy().to_string(),
        size: ByteSize::b(meta.len()),
    })
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct LocalTaskExecutionCacheStoreError(
    pub(crate) LocalTaskExecutionCacheStoreErrorInner,
);

impl LocalTaskExecutionCacheStoreError {
    pub fn kind(&self) -> LocalTaskExecutionCacheStoreErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<LocalTaskExecutionCacheStoreErrorInner>> From<T>
    for LocalTaskExecutionCacheStoreError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(
    name(LocalTaskExecutionCacheStoreErrorKind),
    vis(pub),
    repr(u8)
)]
pub(crate) enum LocalTaskExecutionCacheStoreErrorInner {
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
    RmpSerdeEncode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    RmpSerdeDecode(#[from] rmp_serde::decode::Error),

    #[error(transparent)]
    Bs58(#[from] bs58::decode::Error),

    #[error(transparent)]
    Collect(#[from] omni_collector::error::Error),

    #[error(transparent)]
    LastUsedDb(#[from] LocalLastUsedDbError),

    #[error(transparent)]
    ExecutionPlan(#[from] omni_execution_plan::ExecutionPlanProviderError),

    #[error(transparent)]
    TaskContextProvider(#[from] omni_task_context::TaskContextProviderError),

    #[error(transparent)]
    RemoteCacheServiceClient(#[from] RemoteCacheClientError),

    #[error(
        "a loaded workspace context is required to apply `--stale-only`, `--dir` or `--meta` filters while pruning the cache"
    )]
    MissingContext,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewCacheInfo, cache::impls::HybridTaskExecutionCacheStore};
    use bytes::Bytes;
    use derive_new::new;
    use omni_types::{OmniPath, Root};
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
        // Create the project folders
        sys.fs_create_dir_all_async(dir.join("src"))
            .await
            .expect("failed to create project1 src dir");
        sys.fs_create_dir_all_async(dir.join("dist"))
            .await
            .expect("failed to create project1 dist dir");

        // Content
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

    async fn _fixture_inner(projects: &[&str]) -> tempfile::TempDir {
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
        let dir = _fixture_inner(projects).await;
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

    #[derive(new, Debug)]
    struct TaskExecutionInfoStatic {
        task_name: String,
        task_exec: String,
        project_name: String,
        project_dir: PathBuf,
        input_files: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        env_vars: maps::Map<String, String>,
        input_env_cache_keys: Vec<String>,
        pub dependency_digests: Vec<DefaultHash>,
        pub args: Map<String, serde_json::Value>,
    }

    fn task_from_static<'a>(
        task: &'a TaskExecutionInfoStatic,
    ) -> TaskExecutionInfo<'a> {
        TaskExecutionInfo::new(
            task.task_name.as_str(),
            Some(task.task_exec.as_str()),
            None,
            &task.project_name,
            &task.project_dir,
            &task.output_files,
            &task.input_files,
            &task.input_env_cache_keys,
            &task.env_vars,
            &task.dependency_digests,
            &task.args,
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
            task_exec: format!("ls {}", task_name),
            project_name: project_name.to_string(),
            input_files: vec![OmniPath::new("src/**/*.txt")],
            output_files: vec![OmniPath::new("dist/**/*.js")],
            project_dir,
            env_vars: env_vars(),
            input_env_cache_keys: env_cache_keys(),
            dependency_digests: vec![],
            args: Map::default(),
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

    fn cache_store(root: &Path) -> HybridTaskExecutionCacheStore {
        HybridTaskExecutionCacheStore::new(
            root.join(".omni/cache"),
            root,
            RemoteConfig::new_disabled(),
        )
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
            tries: 1,
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
                OmniPath::new_rooted("rootfile.txt", Root::Workspace),
                OmniPath::new_rooted("dist/**/*.js", Root::Project),
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
    async fn test_prune_with_project_name_globs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                project_name_globs: &["project1"],
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_prune_should_prune_if_larger_than() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                larger_than: Some(ByteSize::b(1)),
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_prune_should_not_prune_if_not_larger_than() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                project_name_globs: &["project1"],
                larger_than: Some(ByteSize::b(1000)),
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_some(), "cached output should exist");
    }

    #[tokio::test]
    async fn test_prune_with_task_name_globs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                project_name_globs: &[],
                task_name_globs: &["task"],
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_prune_with_project_and_task_name_globs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                project_name_globs: &["project1"],
                task_name_globs: &["task"],
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output.is_none(), "cached output should not exist");
    }

    #[tokio::test]
    async fn test_prune_with_no_project_name_globs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        cache
            .prune_caches::<()>(&PruneCacheArgs {
                project_name_globs: &[],
                task_name_globs: &[],
                dry_run: false,
                ..Default::default()
            })
            .await
            .expect("failed to prune caches");

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
            cached_output1.unwrap().digest,
            cached_output2.unwrap().digest,
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
                .push(OmniPath::new_rooted("target/**/*.js", Root::Workspace));
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

    /// When many processes/tasks cache the same task (and therefore the same
    /// content digest) at once, every call must succeed and the published
    /// entry must be complete and readable. This reproduces the cache race
    /// (formerly `crs-004`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_cache_same_digest_is_safe() {
        let temp = fixture(&["project1"]).await;
        let root = temp.path().to_path_buf();
        let cache = cache_store(&root);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let root = root.clone();
            handles.push(tokio::spawn(async move {
                let task = task("task", "project1", &root);
                cache
                    .cache(&new_cache_info(
                        Some(&LOGS_CONTENT),
                        task.get().clone(),
                    ))
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }));
        }

        for handle in handles {
            handle
                .await
                .expect("cache task panicked")
                .expect("every concurrent cache call should succeed");
        }

        // The published entry must be complete and its files readable.
        let task = task("task", "project1", &root);
        let cached_output = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached output should exist after concurrent caching");

        for file in cached_output.files.iter() {
            assert_eq!(
                read_bytes(&file.cached_path).await,
                JS_CONTENT.as_bytes(),
                "cached file content should be intact"
            );
        }

        // No staging directories should be left behind.
        let output_dir = root
            .join(".omni/cache")
            .join(path_safe("project1"))
            .join("output");
        let mut entries = tokio::fs::read_dir(&output_dir)
            .await
            .expect("output dir should exist");
        while let Some(entry) =
            entries.next_entry().await.expect("failed to read dir")
        {
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(STAGING_PREFIX),
                "no staging dirs should remain: {}",
                entry.path().display()
            );
        }
    }

    /// In-progress staging directories must be invisible to readers and stats,
    /// even if they momentarily contain a metadata file.
    #[tokio::test]
    async fn test_staging_dirs_are_ignored_by_stats() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), task.get().clone()))
            .await
            .expect("failed to cache");

        let output_dir = dir
            .join(".omni/cache")
            .join(path_safe("project1"))
            .join("output");

        // Find the real published entry.
        let mut entries =
            tokio::fs::read_dir(&output_dir).await.expect("read output");
        let real_entry = entries
            .next_entry()
            .await
            .expect("read entry")
            .expect("a published entry should exist")
            .path();

        // Plant a staging directory that even has a valid metadata file; it
        // must still be ignored purely by its name.
        let staging = output_dir.join(format!("{STAGING_PREFIX}planted"));
        tokio::fs::create_dir_all(&staging)
            .await
            .expect("mk staging");
        tokio::fs::copy(
            real_entry.join(CACHE_OUTPUT_METADATA_FILE),
            staging.join(CACHE_OUTPUT_METADATA_FILE),
        )
        .await
        .expect("copy meta");

        let stats = cache
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .expect("failed to get stats");

        let project = stats
            .projects
            .iter()
            .find(|p| p.project_name == "project1")
            .expect("project1 should be present");

        assert_eq!(
            project.tasks.len(),
            1,
            "the planted staging dir must not be counted as a cache entry"
        );
    }

    /// Prune reclaims orphaned staging directories older than the threshold,
    /// but leaves fresh ones (which may belong to a concurrent publisher).
    #[tokio::test]
    async fn test_prune_reclaims_stale_staging_dirs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);

        let output_dir = dir
            .join(".omni/cache")
            .join(path_safe("project1"))
            .join("output");
        tokio::fs::create_dir_all(&output_dir)
            .await
            .expect("mk output");

        let orphan = output_dir.join(format!("{STAGING_PREFIX}orphan"));
        tokio::fs::create_dir_all(&orphan).await.expect("mk orphan");

        // With a zero threshold, the orphan is considered stale and removed.
        cache
            .prune_stale_staging_dirs(std::time::Duration::ZERO)
            .await;
        assert!(
            !tokio::fs::try_exists(&orphan).await.unwrap(),
            "stale staging dir should be reclaimed"
        );

        // A fresh staging dir must survive a sweep with a real threshold.
        let fresh = output_dir.join(format!("{STAGING_PREFIX}fresh"));
        tokio::fs::create_dir_all(&fresh).await.expect("mk fresh");
        cache
            .prune_stale_staging_dirs(std::time::Duration::from_secs(3600))
            .await;
        assert!(
            tokio::fs::try_exists(&fresh).await.unwrap(),
            "fresh staging dir must not be reclaimed"
        );
    }

    /// Concurrent reads each update the shared last-used-timestamps database.
    /// The exclusive lock plus atomic save must keep the file valid and avoid
    /// lost updates corrupting it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_last_used_updates_keep_db_valid() {
        let temp = fixture(&["project1"]).await;
        let root = temp.path().to_path_buf();
        let cache = cache_store(&root);

        let seed_task = task("task", "project1", &root);
        cache
            .cache(&new_cache_info(
                Some(&LOGS_CONTENT),
                seed_task.get().clone(),
            ))
            .await
            .expect("failed to cache");

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let root = root.clone();
            handles.push(tokio::spawn(async move {
                let task = task("task", "project1", &root);
                cache
                    .get(task.get())
                    .await
                    .map(|o| o.is_some())
                    .map_err(|e| e.to_string())
            }));
        }

        for handle in handles {
            let found = handle
                .await
                .expect("get task panicked")
                .expect("concurrent get should succeed");
            assert!(found, "cached output should exist");
        }

        // The db must still be loadable and report a last-used timestamp.
        let stats = cache
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .expect("last-used db should remain valid");
        let project = stats
            .projects
            .iter()
            .find(|p| p.project_name == "project1")
            .expect("project1 should be present");

        assert_eq!(project.tasks.len(), 1);
        assert!(
            project.tasks[0].last_used_timestamp.is_some(),
            "a last-used timestamp should have been recorded"
        );
    }
}
