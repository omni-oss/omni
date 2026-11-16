use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::SystemTime,
};

use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use maps::{Map, UnorderedMap, unordered_map};
use omni_collector::{CollectConfig, CollectResult, Collector};
use omni_configurations::{
    RemoteCacheConfiguration, RemoteTarget, ScopeConfig,
};
use omni_execution_plan::{
    Call, DefaultExecutionPlanProvider, ExecutionPlanProvider,
};
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
};
use omni_remote_cache_client::{
    DEFAULT_MAX_ARTIFACT_SIZE, DefaultRemoteCacheClient, Placement, PutOutcome,
    RemoteAccessArgs, RemoteCacheClient, RemoteCacheClientError,
};
use omni_task_context::{
    DefaultTaskContextProvider, EnvVars, TaskContextProviderExt,
    TaskHashProvider,
};
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::{
    FsCopyAsync as _, FsCreateDirAllAsync, FsHardLinkAsync as _,
    FsMetadataAsync, FsReadAsync as _, FsRemoveDirAllAsync as _,
    FsWriteAsync as _, impls::RealSys,
};
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tracing::Level;

use crate::{
    CacheStats, CacheStatsArgs, CachedFileOutput, CachedTaskExecution,
    CachedTaskExecutionHash, Context, FileCacheStats, ProjectCacheStats,
    PruneCacheArgs, PrunedCacheEntry, StaleStatus, TaskCacheStats,
    TaskExecutionCacheStore, TaskExecutionInfo, TaskExecutionInfoExt,
    impls::{
        cache_archive::archive,
        last_used_db::{LocalLastUsedDb, LocalLastUsedDbError},
    },
    storage::{
        STAGING_PREFIX, is_internal_entry,
        lock::{
            CacheLockGuard, LAST_USED_LOCK_FILE, PRUNE_LOCK_FILE,
            REPLACE_LOCK_FILE, lock_file_path,
        },
        publish_staged_dir, reclaim_stale_staging_dirs, staging_dir_for,
    },
};

pub use omni_utils::path::{
    has_globs, path_safe, relpath, remove_globs, topmost_dirs,
};

use omni_glob::{GlobOptions, GlobPatterns, include_set};

use omni_hasher::project_dir_hasher::impls::RealDirHasherError;

use super::{
    cache_archive::unarchive_bytes,
    signing_policy::EffectiveVerify,
    verify::{
        SigningPlan, Verdict, plan_shared_uploads, sha256_of, verify_shared,
    },
};

#[derive(Clone, Debug)]
pub struct HybridTaskExecutionCacheStore {
    sys: RealSys,
    cache_dir: PathBuf,
    /// The whole cache directory. Equal to `cache_dir` except in a view, where
    /// `cache_dir` is one partition: digests and the file-hash index always use
    /// this one, so a view hashes exactly what the store does.
    base_dir: PathBuf,
    ws_root_dir: PathBuf,
    remote_config: RemoteConfig,
    /// Lazily-built remote client. Constructing a `reqwest`/rustls client
    /// eagerly loads the OS trust store (native certs), which is pure overhead
    /// when remote caching is disabled or every entry is served locally. It is
    /// only built on first actual remote access via [`Self::client`].
    client: OnceLock<Arc<DefaultRemoteCacheClient>>,
    /// What the access probe reported, asked for at most once per run and
    /// shared by every clone of the store.
    probe: Arc<tokio::sync::OnceCell<RemoteProbe>>,
    /// The local partition this run uses, resolved once (after the probe, for a
    /// remote-enabled run).
    partition: Arc<tokio::sync::OnceCell<LocalPartition>>,
    /// A store over one directory only (a partition or the cache root): it
    /// never resolves partitions itself. Used to walk every partition.
    is_view: bool,
}

/// Where a run keeps its local entries and how far it trusts what it restores.
#[derive(Clone, Debug)]
struct LocalPartition {
    dir: PathBuf,
    /// The credential can write its isolated namespace but not publish, so
    /// what it builds on remote content stays in its own partition.
    untrusted: bool,
}

/// The advisory result of the access probe. The server enforces everything
/// again on each request; this only avoids requests that cannot succeed.
#[derive(Clone, Debug, Default)]
struct RemoteProbe {
    /// The scopes the credential holds. `None` when the probe could not be
    /// completed (the server decides then).
    scopes: Option<Vec<String>>,
    /// The credential was refused (`401`/`403`): no upload can succeed.
    refused: bool,
    server_max_artifact_size: Option<u64>,
    /// The principal's and the effective scope's path text, as the server
    /// reports them (never client input).
    principal_id: Option<String>,
    scope: Option<String>,
}

impl RemoteProbe {
    fn holds(&self, scope: &str) -> bool {
        self.scopes
            .as_ref()
            .is_some_and(|s| s.iter().any(|h| h == scope))
    }

    fn can_write_isolated(&self) -> Option<bool> {
        self.scopes.as_ref().map(|_| {
            self.holds("write:artifacts") || self.holds("publish:artifacts")
        })
    }
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

    /// Upload successful results on a miss. Off unless configured.
    #[new(default)]
    pub push: bool,

    /// Where uploads go.
    #[new(default)]
    pub target: RemoteTarget,

    /// How the isolated scope label is chosen.
    #[new(default)]
    pub scope: ScopeConfig,

    /// The configured size cap in bytes; the server's value can only lower it.
    #[new(default)]
    pub max_artifact_size: Option<u64>,

    /// What is verified and signed with. Resolved from the committed workspace
    /// policy and the local file, never loosened by the latter.
    #[new(default)]
    #[serde(skip)]
    // Not part of the serialized form: never use that as a fingerprint of the
    // policy. Boxed so the enabled config stays small.
    pub signing: Box<super::signing_policy::SigningPolicy>,
}

impl EnabledRemoteConfig {
    pub fn from_configuration(
        rc: &RemoteCacheConfiguration,
        workspace: &omni_configurations::WorkspaceRemoteCacheConfiguration,
    ) -> Self {
        let mut config = Self::new(
            rc.api_base_url.as_str(),
            rc.api_key.as_str(),
            rc.tenant_code.as_str(),
            rc.organization_code.as_str(),
            rc.workspace_code.as_str(),
            rc.environment_code.clone(),
        );
        config.push = rc.push;
        config.target = rc.target;
        config.scope = rc.scope.clone();
        config.max_artifact_size = rc.max_artifact_size.filter(|n| *n > 0);
        // An invalid policy must never weaken verification: it rejects every
        // shared entry and refuses shared uploads until it is fixed.
        config.signing = Box::new(
            match super::signing_policy::resolve_signing(
                workspace,
                &rc.settings(),
            ) {
                Ok(policy) => policy,
                Err(error) => {
                    tracing::error!(%error, "invalid signing configuration; shared remote cache entries are rejected");
                    super::signing_policy::SigningPolicy::fail_closed()
                }
            },
        );
        config
    }

    /// The isolated scope label for this run.
    fn scope_label(&self) -> String {
        super::remote_scope::resolve(&self.scope, &|name| {
            std::env::var(name).ok()
        })
        .0
    }

    /// The cap on one artifact: the configured one (or the default), lowered
    /// by the server's advertised limit, never raised by it.
    fn size_cap(&self, probe: Option<&RemoteProbe>) -> u64 {
        self.max_artifact_size
            .unwrap_or(DEFAULT_MAX_ARTIFACT_SIZE)
            .min(
                probe
                    .and_then(|p| p.server_max_artifact_size)
                    .unwrap_or(u64::MAX),
            )
    }

    /// The environment code used in every URL and in the signed message.
    pub(super) fn env_code(&self) -> &str {
        self.environment_code.as_deref().unwrap_or("default")
    }

    fn access<'a>(
        &'a self,
        placement: Placement,
        label: &'a str,
        cap: u64,
    ) -> RemoteAccessArgs<'a> {
        RemoteAccessArgs {
            api_key: &self.api_key,
            api_base_url: &self.api_base_url,
            env: self.env_code(),
            org: &self.organization_code,
            tenant: &self.tenant_code,
            ws: &self.workspace_code,
            placement,
            scope: Some(label),
            max_artifact_size: Some(cap),
            signature: None,
        }
    }
}

/// A writer that refuses to hold more than `cap` bytes, so an archive that is
/// too large to upload is abandoned instead of buffered whole.
struct CappedWriter {
    buffer: Vec<u8>,
    cap: u64,
}

impl std::io::Write for CappedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if (self.buffer.len() + data.len()) as u64 > self.cap {
            return Err(std::io::Error::other(
                "the archive is larger than the remote cache size cap",
            ));
        }
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
            base_dir: dir.clone(),
            cache_dir: dir,
            ws_root_dir,
            remote_config: remote_config.into(),
            client: OnceLock::new(),
            probe: Arc::new(tokio::sync::OnceCell::new()),
            partition: Arc::new(tokio::sync::OnceCell::new()),
            is_view: false,
        }
    }

    /// A store for tasks that must never touch the remote cache: no remote, and
    /// entries kept in the `local` partition (RFC 0030, section 4.4). Digests
    /// and the file-hash index still use the whole cache directory.
    pub fn new_local_only(
        cache_dir: impl Into<PathBuf>,
        ws_root_dir: impl Into<PathBuf>,
    ) -> Self {
        let store = Self::new(cache_dir, ws_root_dir, RemoteConfig::Disabled);
        let local = LocalPartition {
            dir: super::partition::local_dir(&store.base_dir),
            untrusted: false,
        };
        store
            .partition
            .set(local)
            .expect("a new store has not resolved a partition");
        store
    }

    /// A store over exactly `dir`, with no remote and no partitions.
    fn view(&self, dir: PathBuf) -> Self {
        Self {
            cache_dir: dir,
            remote_config: RemoteConfig::Disabled,
            is_view: true,
            ..self.clone()
        }
    }

    /// The directories this store covers: the cache root as a plain layout and
    /// every partition below it. Stats and prune walk all of them so nothing
    /// is invisible or unprunable, and no network call is needed.
    async fn views(&self) -> Vec<Self> {
        if self.is_view {
            return vec![self.clone()];
        }
        let mut views = vec![self.view(self.cache_dir.clone())];
        for dir in super::partition::existing_partitions(&self.cache_dir).await
        {
            views.push(self.view(dir));
        }
        views
    }

    /// The partition of this run. Remote-disabled stores, and views, use the
    /// cache directory as it is; a remote-enabled run asks the server what its
    /// credential may do first (that answer is also what the probe is for), and
    /// anything unknown is trusted, the strict case.
    async fn local_partition(&self) -> &LocalPartition {
        self.partition
            .get_or_init(|| async {
                let RemoteConfig::Enabled(conf) = &self.remote_config else {
                    return LocalPartition {
                        dir: self.cache_dir.clone(),
                        untrusted: false,
                    };
                };
                if self.is_view {
                    return LocalPartition {
                        dir: self.cache_dir.clone(),
                        untrusted: false,
                    };
                }
                let probe = self.probe(conf).await;
                let trusted_ids = conf.signing.trusted_ids();
                let epoch = super::partition::trust_epoch(
                    &super::partition::EpochInputs {
                        api_base_url: &conf.api_base_url,
                        tenant: &conf.tenant_code,
                        org: &conf.organization_code,
                        ws: &conf.workspace_code,
                        env: conf.environment_code.as_deref(),
                        trusted_key_ids: &trusted_ids,
                        verify_mode: conf.signing.verify.as_str(),
                    },
                );
                let untrusted = !probe.refused
                    && probe.scopes.is_some()
                    && probe.holds("write:artifacts")
                    && !probe.holds("publish:artifacts");
                if untrusted
                    && let (Some(principal), Some(scope)) =
                        (&probe.principal_id, &probe.scope)
                {
                    return LocalPartition {
                        dir: super::partition::untrusted_dir(
                            &self.base_dir,
                            &epoch,
                            principal,
                            scope,
                        ),
                        untrusted: true,
                    };
                }
                if untrusted {
                    // Known untrusted, but with no partition name to use: keep
                    // what it builds where no trusted run reads it.
                    tracing::warn!(
                        "the server did not report this credential's principal and scope; using a separate local cache partition"
                    );
                    return LocalPartition {
                        dir: super::partition::unknown_dir(&self.base_dir, &epoch),
                        untrusted: true,
                    };
                }
                LocalPartition {
                    dir: super::partition::trusted_dir(&self.base_dir, &epoch),
                    untrusted: false,
                }
            })
            .await
    }

    /// Return the remote cache client, building it on first use. This defers the
    /// (relatively expensive) TLS trust-store load until a remote request is
    /// actually made.
    fn client(&self) -> &Arc<DefaultRemoteCacheClient> {
        self.client
            .get_or_init(|| Arc::new(DefaultRemoteCacheClient::default()))
    }

    /// Asks the server once what this credential may do. A transport or server
    /// failure is remembered as unknown (no retry for every task); a refused
    /// credential is remembered as refused.
    async fn probe(&self, conf: &EnabledRemoteConfig) -> &RemoteProbe {
        self.probe
            .get_or_init(|| async {
                let (label, warning) = super::remote_scope::resolve(
                    &conf.scope,
                    &|name| std::env::var(name).ok(),
                );
                if let Some(warning) = warning {
                    tracing::warn!("{warning}");
                }
                let access = conf.access(
                    Placement::Isolated,
                    &label,
                    conf.size_cap(None),
                );
                match self.client().validate_access(&access).await {
                    Ok(result) if result.is_valid => RemoteProbe {
                        scopes: (!result.scopes.is_empty())
                            .then_some(result.scopes),
                        refused: false,
                        server_max_artifact_size: result.max_artifact_size,
                        principal_id: result.principal_id,
                        scope: result.scope,
                    },
                    // Only a refusal means no upload can succeed. A server
                    // error, a `404` from a server without the probe or a `429`
                    // leaves it to the server to decide per request.
                    Ok(result) if matches!(result.status, Some(401 | 403)) => {
                        tracing::warn!(
                            message = result.message.as_deref().unwrap_or(""),
                            "the remote cache refused this credential; not uploading"
                        );
                        RemoteProbe {
                            refused: true,
                            ..Default::default()
                        }
                    }
                    Ok(result) => {
                        tracing::debug!(status = ?result.status, "remote cache access probe was not answered with a verdict; the server decides per request");
                        RemoteProbe::default()
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "remote cache access probe failed; the server decides per request");
                        RemoteProbe::default()
                    }
                }
            })
            .await
    }
}

/// Restores a remote archive into `entry_dir` through a sibling staging
/// directory: the marker is written before the directory is renamed into place,
/// so a crash can never leave a remote entry without its marker, and a partial
/// extraction never lands in the final directory. A rejected archive
/// (`InvalidData`) leaves nothing behind.
async fn restore_staged(
    entry_dir: &Path,
    bytes: Bytes,
    marker: super::origin::Marker,
) -> std::io::Result<()> {
    let staging = staging_dir_for(entry_dir);
    let extract = {
        let staging = staging.clone();
        tokio::task::spawn_blocking(move || unarchive_bytes(&staging, &bytes))
            .await
            .map_err(std::io::Error::other)
            .and_then(|result| result)
    };
    let staged = async {
        extract?;
        tokio::fs::write(
            staging.join(super::origin::ORIGIN_MARKER),
            marker.text(),
        )
        .await?;
        publish_staged_dir(
            &staging,
            entry_dir,
            &entry_dir.join(CACHE_OUTPUT_METADATA_FILE),
            false,
        )
        .await
    }
    .await;
    if staged.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    staged
}

/// True when every path in a decoded cache entry can only descend from the base
/// it is later joined onto: `cached_path` and `logs_path` from the entry
/// directory; `original_path` must stay inside the workspace once `..` is
/// resolved (an output in a sibling project directory is legitimate).
fn restore_paths_are_confined(
    entry: &CachedTaskExecution,
    roots: &omni_types::RootMap,
) -> bool {
    entry
        .logs_path
        .as_deref()
        .is_none_or(omni_types::is_confined_relative)
        && entry.files.iter().all(|file| {
            omni_types::is_confined_relative(&file.cached_path)
                && file.original_path.stays_within_workspace(roots)
        })
}

/// True only for a regular file. Unlike an existence check this does not follow
/// a symlink, so a link that an entry smuggled in is a miss and never a source
/// that restore would read from or copy.
async fn is_regular_file(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
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
        let call = if task_name_globs.is_empty() {
            Call::all_tasks()
        } else {
            Call::pattern_tasks(task_name_globs.iter().copied())
        };

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

    #[tracing::instrument(level = Level::DEBUG, skip(self, tasks, config))]
    async fn collect<'a>(
        &'a self,
        tasks: &[TaskExecutionInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, LocalTaskExecutionCacheStoreError> {
        let collect_task_infos = tasks
            .iter()
            .map(|task| omni_collector::ProjectTaskInfo {
                input_files: task.input_files,
                input_files_exclude: task.input_files_exclude,
                output_files: task.output_files,
                output_files_exclude: task.output_files_exclude,
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

        let partition = self.local_partition().await;
        let to_process = Collector::new(
            self.ws_root_dir.as_path(),
            self.base_dir.as_path(),
            self.sys.clone(),
        )
        .with_output_base(partition.dir.as_path())
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
        let dir = &self.local_partition().await.dir;
        let _guard = CacheLockGuard::acquire_exclusive(lock_file_path(
            dir,
            LAST_USED_LOCK_FILE,
        ))
        .await?;

        let path = dir.join(LAST_USED_TIMESTAMPS_DB_FILE);
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

    /// Reads the recorded exit code of a published cache entry.
    ///
    /// Returns `None` when no complete entry is present at `metadata_path`.
    /// The exit code is used to decide whether a freshly-produced result may
    /// replace an existing entry: entries are addressed only by their input
    /// digest, which does not encode the execution outcome, so a cached
    /// failure and a later success of the same flaky task resolve to the same
    /// location.
    async fn read_cached_exit_code(
        &self,
        metadata_path: &Path,
    ) -> Result<Option<u32>, LocalTaskExecutionCacheStoreError> {
        if !self.sys.fs_exists_async(metadata_path).await? {
            return Ok(None);
        }

        let bytes = self.sys.fs_read_async(metadata_path).await?;
        let metadata: CachedTaskExecution =
            rmp_serde::decode::from_slice(&bytes)?;

        Ok(Some(metadata.exit_code))
    }

    async fn can_upload_cached_result(
        &self,
        caller_exit_code: u32,
        metadata_path: &Path,
    ) -> bool {
        // A racing first publisher can win with a failure even when this
        // caller succeeded. Only persisted successes may cross the boundary.
        // Published successes cannot be downgraded by another local writer.
        if !self.remote_config.is_enabled() || caller_exit_code != 0 {
            return false;
        }
        match self.read_cached_exit_code(metadata_path).await {
            Ok(exit_code) => exit_code == Some(0),
            Err(e) => {
                tracing::warn!(error = %e, "Skipping remote upload; could not verify persisted outcome");
                false
            }
        }
    }

    /// Removes the given cache entries from disk without taking the prune
    /// lock. Callers must already hold the exclusive prune lock.
    async fn force_prune_inner(
        &self,
        entries: &[PrunedCacheEntry],
    ) -> Result<(), LocalTaskExecutionCacheStoreError> {
        for entry in entries {
            if !tokio::fs::try_exists(&entry.entry_dir).await? {
                tracing::debug!(
                    "Cache entry does not exist: {}",
                    entry.entry_dir.display()
                );
                continue;
            }

            tokio::fs::remove_dir_all(&entry.entry_dir)
                .await
                .inspect_err(|e| {
                    tracing::error!("Failed to delete cache entry: {}", e)
                })?;

            tracing::debug!(
                "Pruned cache entry: {}",
                entry.entry_dir.display()
            );
        }

        Ok(())
    }

    /// Reclaims staging directories left behind by crashed processes.
    ///
    /// Only directories older than `max_age` are removed, so a
    /// concurrently-publishing peer's fresh staging directory is never
    /// touched. Best-effort: individual failures are logged and skipped.
    async fn prune_stale_staging_dirs(&self, max_age: std::time::Duration) {
        for view in self.views().await {
            let mut projects = match tokio::fs::read_dir(&view.cache_dir).await
            {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            while let Ok(Some(project)) = projects.next_entry().await {
                reclaim_stale_staging_dirs(
                    &project.path().join("output"),
                    max_age,
                )
                .await;
            }
        }
    }
}

const LOGS_CACHE_FILE: &str = "logs.cache";
const CACHE_OUTPUT_METADATA_FILE: &str = "cache.meta.bin";
const LAST_USED_TIMESTAMPS_DB_FILE: &str = "last-used-timestamps.db";

/// Staging directories older than this are considered orphaned (left behind by
/// a crashed process) and are reclaimed during prune.
const STALE_STAGING_AGE: std::time::Duration =
    std::time::Duration::from_secs(60 * 60);

#[async_trait::async_trait]
impl TaskExecutionCacheStore for HybridTaskExecutionCacheStore {
    type Error = LocalTaskExecutionCacheStoreError;

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    async fn cache_many<'a>(
        &'a self,
        cache_infos: &[crate::NewCacheInfo<'a>],
    ) -> Result<Vec<CachedTaskExecutionHash<'a>>, Self::Error> {
        tracing::trace!(count = cache_infos.len(), "begin_cache_many");

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

        tracing::trace!(
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
        let local_dir = self.local_partition().await.dir.clone();
        let _prune_guard = CacheLockGuard::acquire_shared(lock_file_path(
            &local_dir,
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

            let tfqn = format!(
                "{}#{}",
                result.task.project_name, result.task.task_name
            );
            let new_cache_info =
                cache_info_map.get(&tfqn).expect("should be some");

            // Entries are addressed only by their input digest, which does not
            // encode the execution outcome. A flaky task can therefore produce
            // a failed entry and, on a later `-f=failed` re-run, a successful
            // one at the same content-addressed location. A success must be
            // allowed to replace a previously cached failure; a failure must
            // never overwrite a success, and identical outcomes are left as-is
            // (reusing an existing entry avoids racing peers reading it).
            let existing_exit_code =
                self.read_cached_exit_code(&metadata_path).await?;

            // Overwriting an existing entry is only ever needed to upgrade a
            // cached failure to a success. Serialize those replacements with a
            // dedicated exclusive lock so two concurrent upgrades of the same
            // digest cannot race, then re-read under the lock so we never
            // downgrade an entry a peer just upgraded.
            let upgrading = existing_exit_code.is_some_and(|c| c != 0)
                && new_cache_info.exit_code == 0;

            let _replace_guard = if upgrading {
                Some(
                    CacheLockGuard::acquire_exclusive(lock_file_path(
                        &local_dir,
                        REPLACE_LOCK_FILE,
                    ))
                    .await?,
                )
            } else {
                None
            };

            let existing_exit_code = if upgrading {
                self.read_cached_exit_code(&metadata_path).await?
            } else {
                existing_exit_code
            };

            let should_write = match existing_exit_code {
                // Nothing cached yet: write a fresh entry.
                None => true,
                // A cached success is authoritative and never overwritten.
                Some(0) => false,
                // A cached failure is replaced only by a success.
                Some(_) => new_cache_info.exit_code == 0,
            };
            let replacing = should_write && existing_exit_code.is_some();

            if should_write {
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

                        if let Err(err) = self.sys
                            .fs_hard_link_async(
                                &original_abs_path,
                                &cache_abs_file_path,
                            )
                            .await
                        {
                            if !omni_utils::fs::should_copy_after_hardlink_error(&err) {
                                return Err(err.into());
                            }
                            tracing::debug!(
                                "Saving cached file by copy because hardlinking was unavailable ({err}): {original_abs_path:?} to {cache_abs_file_path:?}"
                            );
                            self.sys.fs_copy_async(&original_abs_path, &cache_abs_file_path).await?;
                        } else {
                            tracing::debug!(
                                "cache file hard link {original_abs_path:?} to {cache_abs_file_path:?}"
                            );
                        }

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
                        task_exec: result
                            .task
                            .task_exec
                            .map(|c| c.canonical().into_owned()),
                        task_retry_exec: result
                            .task
                            .task_retry_exec
                            .map(|c| c.canonical().into_owned()),
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

                publish_staged_dir(
                    &staging_dir,
                    output_dir,
                    &metadata_path,
                    replacing,
                )
                .await?;
            }

            let execution_hash = CachedTaskExecutionHash {
                project_name: result.task.project_name,
                task_name: result.task.task_name,
                digest: result.digest.expect("should be some"),
            };

            cache_exec_hashes.push(execution_hash);

            if self
                .can_upload_cached_result(
                    new_cache_info.exit_code,
                    &metadata_path,
                )
                .await
            {
                cached_results.push((execution_hash, output_dir.to_path_buf()));
            }
        }

        if !cached_results.is_empty()
            && let RemoteConfig::Enabled(conf) = &self.remote_config
            && conf.push
        {
            let probe = self.probe(conf).await.clone();
            let needed = match conf.target {
                RemoteTarget::Shared => "publish:artifacts",
                RemoteTarget::Isolated => {
                    "write:artifacts or publish:artifacts"
                }
            };
            let allowed = !probe.refused
                && match (conf.target, probe.scopes.is_some()) {
                    (_, false) => true,
                    (RemoteTarget::Shared, true) => {
                        probe.holds("publish:artifacts")
                    }
                    (RemoteTarget::Isolated, true) => {
                        probe.can_write_isolated() == Some(true)
                    }
                };
            if !allowed {
                tracing::warn!(
                    needed,
                    "this credential cannot upload to the configured target; skipping remote cache uploads"
                );
                return Ok(cache_exec_hashes);
            }

            // A shared entry is signed when this machine has a key. A workspace
            // that trusts keys never gets an unsigned shared upload from here.
            let signer = if conf.target == RemoteTarget::Shared {
                match plan_shared_uploads(&conf.signing) {
                    SigningPlan::Unsigned => None,
                    SigningPlan::Sign(handle) => Some(handle),
                    SigningPlan::Refuse(reason) => {
                        tracing::warn!(%reason, "not uploading to the shared remote cache");
                        return Ok(cache_exec_hashes);
                    }
                }
            } else {
                None
            };

            tracing::debug!("Uploading remote cache artifacts...");
            let mut tasks = JoinSet::new();
            // Bound archive buffers as well as network requests. Acquire inside
            // each worker before building the archive, not just before PUT.
            let upload_slots = Arc::new(tokio::sync::Semaphore::new(8));
            let cap = conf.size_cap(Some(&probe));
            let label = conf.scope_label();
            for (hash, output_dir) in cached_results {
                let client = self.client().clone();
                let conf = conf.clone();
                let label = label.clone();
                let digest = bs58::encode(hash.digest).into_string();
                let upload_slots = upload_slots.clone();
                let signer = signer.clone();

                tasks.spawn(async move {
                    let _permit = upload_slots
                        .acquire_owned()
                        .await
                        .expect("upload semaphore is never closed");
                    let placement = match conf.target {
                        RemoteTarget::Shared => Placement::Shared,
                        RemoteTarget::Isolated => Placement::Isolated,
                    };
                    let upload = async {
                        // No existence pre-check: the server stores entries
                        // create-only and answers `409` for an existing one,
                        // which the client treats as success. A `HEAD` would
                        // need read access and would race the write anyway.
                        let mut artifact = CappedWriter {
                            buffer: Vec::new(),
                            cap,
                        };
                        archive(&output_dir, &mut artifact)?;

                        let artifact = Bytes::from(artifact.buffer);
                        let signature = match &signer {
                            Some(signer) => {
                                let hash = sha256_of(artifact.clone())
                                    .await
                                    .map_err(std::io::Error::other)?;
                                Some(signer.sign(&omni_remote_cache_client::signed_message(
                                    &conf.tenant_code,
                                    &conf.organization_code,
                                    &conf.workspace_code,
                                    conf.env_code(),
                                    &digest,
                                    &hash,
                                )))
                            }
                            None => None,
                        };
                        let mut config = conf.access(placement, &label, cap);
                        config.signature = signature.as_deref();

                        let outcome = client
                            .put_artifact_outcome(&config, &digest, artifact)
                            .await?;

                        // Entries are immutable, so an existing shared entry
                        // stays. A publisher checks that it is a good one and
                        // says so if it is not, since nothing else will.
                        if outcome == PutOutcome::AlreadyExists
                            && placement == Placement::Shared
                            && conf.signing.verify != EffectiveVerify::Off
                        {
                            let read = conf.access(Placement::Shared, &label, cap);
                            match client.get_artifact_entry(&read, &digest).await {
                                Ok(Some(existing)) => {
                                    if let Verdict::Failed(reason) =
                                        verify_shared(&conf, &digest, &existing).await
                                    {
                                        tracing::warn!(%digest, %reason, "the shared entry already stored for this digest does not verify; purge it so it can be published again");
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => tracing::debug!(%digest, error = %e, "could not check the existing shared entry"),
                            }
                        }

                        Ok::<_, LocalTaskExecutionCacheStoreError>(())
                    }
                    .await;

                    match upload {
                        Ok(()) => tracing::debug!(%digest, "Remote cache upload accepted"),
                        Err(e) => tracing::warn!(%digest, error = %e, "Remote cache upload failed; local result retained"),
                    }
                });
            }

            // Join each task explicitly: join_all panics on a failed worker.
            // Optional remote publication must not fail successful local caching.
            while let Some(result) = tasks.join_next().await {
                if let Err(e) = result {
                    tracing::warn!(error = %e, "Remote cache upload worker failed; local results retained");
                }
            }
        }

        Ok(cache_exec_hashes)
    }

    #[tracing::instrument(level = Level::DEBUG, skip(self, projects))]
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
            // Bound the archives held in memory, like uploads.
            let download_slots = Arc::new(tokio::sync::Semaphore::new(8));
            let label = conf.scope_label();
            // The partition (and so the probe) is settled before any local
            // entry is looked at or written.
            let partition = self.local_partition().await;
            let untrusted = partition.untrusted;
            let probe = self.probe(conf).await.clone();
            // Restores and removals share the prune lock like publishers do, so
            // a prune never deletes an entry that is being renamed into place.
            let _prune_guard = CacheLockGuard::acquire_shared(lock_file_path(
                &partition.dir,
                PRUNE_LOCK_FILE,
            ))
            .await?;
            for project in &collected {
                let output_dir = project
                    .cache_output_dir
                    .as_deref()
                    .expect("should be some");

                // A trusted run never reuses an entry that came from an
                // isolated namespace (or whose marker cannot be read). It
                // cannot be here unless a cache directory was shared against
                // the guidance, and it is removed so the entry is rebuilt or
                // fetched like a miss.
                let marker = super::origin::read_marker(output_dir).await;
                if !untrusted
                    && marker.as_ref().map(|m| m.origin)
                        == Some(super::origin::Origin::Isolated)
                {
                    tracing::warn!(
                        project = %project.task.project_name,
                        "discarding a local entry restored from an isolated remote namespace"
                    );
                    tokio::fs::remove_dir_all(output_dir).await?;
                } else if conf.signing.verify == EffectiveVerify::Require
                    && let Some(m) = &marker
                    && m.origin == super::origin::Origin::Shared
                    && !m.verified_by.as_ref().is_some_and(|id| {
                        conf.signing.trusted.iter().any(|k| k.id() == id)
                    })
                {
                    // Restored under a weaker policy, or signed by a key that
                    // is no longer trusted: not reusable under `require`.
                    tracing::warn!(
                        project = %project.task.project_name,
                        "discarding a local remote entry that was not verified by a currently trusted key"
                    );
                    tokio::fs::remove_dir_all(output_dir).await?;
                }

                // skip if the output dir already exists/downloaded
                if tokio::fs::try_exists(output_dir).await? {
                    continue;
                }

                let digest =
                    bs58::encode(project.digest.unwrap()).into_string();

                let client = self.client().clone();
                let conf = conf.clone();
                let output_dir = output_dir.to_path_buf();
                let download_slots = download_slots.clone();
                let probe = probe.clone();
                let label = label.clone();

                tasks.spawn(async move {
                    let _permit = download_slots
                        .acquire_owned()
                        .await
                        .expect("download semaphore is never closed");
                    let cap = conf.size_cap(Some(&probe));
                    // Only an untrusted run reads its own isolated entries
                    // (it holds `write` but not `publish`). A run that can
                    // publish never requests them: what it restores is shared
                    // and, once signing lands, verified.
                    let placements: &[Placement] = if untrusted
                        && conf.target == RemoteTarget::Isolated
                    {
                        &[Placement::Isolated, Placement::Shared]
                    } else {
                        &[Placement::Shared]
                    };
                    // A failed read, a refused one (`403`) or an oversized or
                    // truncated body is a miss: the remote is optional and must
                    // not fail the run.
                    let mut response = None;
                    for &placement in placements {
                        let access = conf.access(placement, &label, cap);
                        match client.get_artifact_entry(&access, &digest).await
                        {
                            Ok(Some(artifact)) => {
                                response = Some((artifact, placement));
                                break;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::debug!(
                                    %digest,
                                    ?placement,
                                    error = %e,
                                    "remote cache read failed; treating it as a miss"
                                );
                            }
                        }
                    }

                    if let Some((artifact, placement)) = response {
                        tracing::debug!("fetched remote cache for {}", digest);
                        // Shared entries are checked before a single byte is
                        // extracted. Isolated ones are unsigned by design: only
                        // an untrusted run ever reads them.
                        let verified_by = if placement == Placement::Shared {
                            match verify_shared(&conf, &digest, &artifact).await {
                                Verdict::Verified(id) => Some(id),
                                Verdict::Skipped => None,
                                Verdict::Failed(reason) => {
                                    match conf.signing.verify {
                                        EffectiveVerify::Require => {
                                            tracing::warn!(%digest, %reason, "rejecting a remote cache entry that failed verification");
                                            return Ok(());
                                        }
                                        _ => {
                                            tracing::warn!(%digest, %reason, "restoring a remote cache entry that failed verification (verify: warn)");
                                            None
                                        }
                                    }
                                }
                            }
                        } else {
                            None
                        };
                        // `verified_by` records only a verification that
                        // `require` depends on; `warn` and `off` write `none`.
                        let marker = super::origin::Marker {
                            origin: match placement {
                                Placement::Shared => {
                                    super::origin::Origin::Shared
                                }
                                Placement::Isolated => {
                                    super::origin::Origin::Isolated
                                }
                            },
                            verified_by: verified_by
                                .filter(|_| conf.signing.verify == EffectiveVerify::Require),
                        };
                        match restore_staged(&output_dir, artifact.bytes, marker)
                            .await
                        {
                            Ok(()) => {}
                            // Rejected before anything was written (a link, an
                            // escaping name, or a corrupt archive): a miss.
                            Err(e)
                                if e.kind()
                                    == std::io::ErrorKind::InvalidData =>
                            {
                                tracing::warn!(
                                    %digest,
                                    error = %e,
                                    "ignoring an unsafe or corrupt remote cache entry"
                                );
                            }
                            Err(e) => return Err(e.into()),
                        }
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

                // An entry may come from a remote cache or a hostile writer, and
                // its metadata is not covered by the archive layer's checks. A
                // path that is absolute, uses `..`, or is not anchored to a
                // root would be joined onto a base and could read or write
                // outside it, so the whole entry is a miss.
                if !restore_paths_are_confined(&cached_output, &project.roots) {
                    tracing::warn!(
                        project = %project.task.project_name,
                        "ignoring a cache entry with an unsafe path in its metadata"
                    );
                    outputs.push(None);
                    continue 'outer_loop;
                }

                // canonicalize the paths
                if let Some(logs_path) = cached_output.logs_path.as_mut() {
                    let p = cache_abs(logs_path)?;

                    if !is_regular_file(&p).await {
                        outputs.push(None);
                        continue 'outer_loop;
                    }

                    *logs_path = p;
                }

                for file in cached_output.files.iter_mut() {
                    let c = cache_abs(&file.cached_path)?;

                    if !is_regular_file(&c).await {
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
        if !self.is_view {
            let mut merged = CacheStats::default();
            for view in self.views().await {
                let part = view.get_stats(args).await?;
                for project in part.projects {
                    match merged
                        .projects
                        .iter_mut()
                        .find(|p| p.project_name == project.project_name)
                    {
                        Some(existing) => existing.tasks.extend(project.tasks),
                        None => merged.projects.push(project),
                    }
                }
            }
            return Ok(merged);
        }

        let project_name_globs = args.project_name_globs;
        let task_name_globs = args.task_name_globs;

        let mut entries = tokio::fs::read_dir(&self.cache_dir).await?;

        let project_glob = {
            let project_name_globs: &[&str] = if project_name_globs.is_empty() {
                &["*"]
            } else {
                project_name_globs
            };
            include_set(project_name_globs, GlobOptions::default())?
        };
        let task_glob = {
            let task_name_globs: &[&str] = if task_name_globs.is_empty() {
                &["*"]
            } else {
                task_name_globs
            };
            include_set(task_name_globs, GlobOptions::default())?
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
            if is_internal_entry(&enc_project_name) {
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
                    tracing::debug!(
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
                        tracing::debug!(
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
                        tracing::debug!(
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
        if !self.is_view {
            // Each partition is a store of its own with its own lock, so a
            // prune holds exactly the lock of the entries it deletes.
            let mut pruned = Vec::new();
            for view in self.views().await {
                pruned.extend(view.prune_caches(args).await?);
            }
            return Ok(pruned);
        }

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
        let time_upper_limit = args
            .older_than
            .map(|older_than| OffsetDateTime::now_utc() - older_than);

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

            let call = if args.task_name_globs.is_empty() {
                Call::all_tasks()
            } else {
                Call::pattern_tasks(args.task_name_globs.iter().copied())
            };
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
                tracing::debug!(
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
        if !self.is_view {
            // The owner of an entry is the deepest view that contains it.
            let views = self.views().await;
            for entry in entries {
                if let Some(owner) = views
                    .iter()
                    .filter(|v| entry.entry_dir.starts_with(&v.cache_dir))
                    .max_by_key(|v| v.cache_dir.components().count())
                {
                    owner
                        .force_prune_caches(std::slice::from_ref(entry))
                        .await?;
                }
            }
            return Ok(());
        }

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
    ) -> &GlobPatterns<OmniPath> {
        match self.inner.get_cache_info(project_name, task_name) {
            Some(c) => &c.key_input_files,
            None => omni_execution_plan::empty_cache_input_files(),
        }
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

    fn get_output_logs(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration> {
        self.inner.get_output_logs(project_name, task_name)
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
        tracing::error!(
            "failed to get metadata for {}: {}",
            path_ref.display(),
            e
        )
    })?;

    Ok::<_, LocalTaskExecutionCacheStoreError>(FileCacheStats {
        path: path_ref.to_string_lossy().to_string(),
        size: ByteSize::b(meta.len()),
    })
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(
    name(LocalTaskExecutionCacheStoreErrorKind),
    vis(pub),
    repr(u8)
)]
pub(crate) enum LocalTaskExecutionCacheStoreErrorInner {
    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    DirHasher(#[from] RealDirHasherError),

    #[error(transparent)]
    #[diagnostic(skip)]
    Globset(#[from] globset::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Ignore(#[from] dir_walker::impls::IgnoreError),

    #[error(transparent)]
    #[diagnostic(skip)]
    IgnoreBuild(#[from] dir_walker::impls::IgnoreRealDirWalkerError),

    #[error(transparent)]
    #[diagnostic(skip)]
    RmpSerdeEncode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    RmpSerdeDecode(#[from] rmp_serde::decode::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Bs58(#[from] bs58::decode::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Collect(#[from] omni_collector::error::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    LastUsedDb(#[from] LocalLastUsedDbError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExecutionPlan(#[from] omni_execution_plan::ExecutionPlanProviderError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    TaskContextProvider(#[from] omni_task_context::TaskContextProviderError),

    #[error(transparent)]
    #[diagnostic(skip)]
    RemoteCacheServiceClient(#[from] RemoteCacheClientError),

    #[error(
        "a loaded workspace context is required to apply `--stale-only`, `--dir` or `--meta` filters while pruning the cache"
    )]
    #[diagnostic(skip)]
    MissingContext,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{NewCacheInfo, cache::impls::HybridTaskExecutionCacheStore};
    use bytes::Bytes;
    use derive_new::new;
    use omni_command_config::CommandConfig;
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

    #[allow(clippy::too_many_arguments)]
    #[derive(new, Debug)]
    struct TaskExecutionInfoStatic {
        task_name: String,
        task_exec: CommandConfig,
        project_name: String,
        project_dir: PathBuf,
        input_files: Vec<OmniPath>,
        input_files_exclude: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        output_files_exclude: Vec<OmniPath>,
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
            Some(&task.task_exec),
            None,
            &task.project_name,
            &task.project_dir,
            &task.output_files,
            &task.output_files_exclude,
            &task.input_files,
            &task.input_files_exclude,
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
            task_exec: CommandConfig::Shell(format!("ls {}", task_name)),
            project_name: project_name.to_string(),
            input_files: vec![OmniPath::new("src/**/*.txt")],
            input_files_exclude: vec![],
            output_files: vec![OmniPath::new("dist/**/*.js")],
            output_files_exclude: vec![],
            project_dir,
            env_vars: env_vars(),
            input_env_cache_keys: env_cache_keys(),
            dependency_digests: vec![],
            args: Map::default(),
        };
        f(&mut owned);

        Yoke::attach_to_cart(Box::new(owned), task_from_static)
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

    fn new_cache_info_with_exit_code<'a>(
        logs: Option<&'a Bytes>,
        task: TaskExecutionInfo<'a>,
        exit_code: u32,
    ) -> NewCacheInfo<'a> {
        NewCacheInfo {
            logs,
            task,
            exit_code,
            execution_duration: std::time::Duration::from_millis(100),
            tries: 1,
        }
    }

    struct RemoteUploadSpy {
        url: String,
        /// Every request except the access probe.
        requests: Arc<std::sync::Mutex<Vec<(String, Bytes)>>>,
        /// The request line of every request, probe included.
        lines: Arc<std::sync::Mutex<Vec<String>>>,
        /// The whole request head of every request, probe included.
        heads: Arc<std::sync::Mutex<Vec<String>>>,
        server: tokio::task::JoinHandle<()>,
        push: bool,
        target: RemoteTarget,
        /// Extra probe response headers, each ending in CRLF.
        probe_headers: Arc<std::sync::Mutex<String>>,
    }

    impl RemoteUploadSpy {
        async fn new(put_status: u16) -> Self {
            Self::with_scopes(put_status, "write:artifacts").await
        }

        /// A server whose probe reports `scopes`.
        async fn with_scopes(put_status: u16, scopes: &'static str) -> Self {
            use tokio::io::AsyncWriteExt;
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = requests.clone();
            let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
            let seen_lines = lines.clone();
            let heads = Arc::new(std::sync::Mutex::new(Vec::new()));
            let seen_heads = heads.clone();
            let probe_headers = Arc::new(std::sync::Mutex::new(String::new()));
            let probe_extra = probe_headers.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        header.push(socket.read_u8().await.unwrap());
                        assert!(header.len() < 32 * 1024);
                    }
                    let header = String::from_utf8(header).unwrap();
                    let line = header.lines().next().unwrap().to_string();
                    seen_lines.lock().unwrap().push(line.clone());
                    seen_heads.lock().unwrap().push(header.clone());
                    if line.starts_with("HEAD /v1/artifacts?") {
                        let extra = probe_extra.lock().unwrap().clone();
                        let response = format!(
                            "HTTP/1.1 204 No Content\r\nx-omni-scopes: {scopes}\r\n{extra}Connection: close\r\n\r\n"
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                        continue;
                    }
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    assert!(length <= 1024 * 1024);
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).await.unwrap();
                    let method = header.split_whitespace().next().unwrap();
                    if method == "PUT" {
                        // The server seals the entry only when the bytes match
                        // this header, so an upload must always declare its hash.
                        use sha2::{Digest as _, Sha256};
                        let declared: Vec<&str> = header
                            .lines()
                            .filter_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case(
                                    "x-omni-content-sha256",
                                )
                                .then(|| value.trim())
                            })
                            .collect();
                        let actual: String = Sha256::digest(&body)
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect();
                        assert_eq!(declared, [actual.as_str()]);
                    }
                    let status = match method {
                        "PUT" => put_status,
                        // A write-only key is denied every read request.
                        _ => 403,
                    };
                    captured
                        .lock()
                        .unwrap()
                        .push((method.to_string(), Bytes::from(body)));
                    if status == 0 {
                        // Simulate a transport failure after receiving the body.
                        continue;
                    }
                    let response = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            });
            Self {
                url,
                requests,
                lines,
                heads,
                server,
                push: true,
                target: RemoteTarget::Isolated,
                probe_headers,
            }
        }

        fn cache(&self, root: &Path) -> HybridTaskExecutionCacheStore {
            HybridTaskExecutionCacheStore::new(
                root.join(".omni/cache"),
                root,
                RemoteConfig::Enabled(EnabledRemoteConfig {
                    api_base_url: self.url.clone(),
                    api_key: "test".to_string(),
                    tenant_code: "t".to_string(),
                    organization_code: "o".to_string(),
                    workspace_code: "w".to_string(),
                    environment_code: Some("e".to_string()),
                    push: self.push,
                    target: self.target,
                    scope: ScopeConfig::Literal("pr-1".to_string()),
                    max_artifact_size: None,
                    signing: Default::default(),
                }),
            )
        }
    }

    impl Drop for RemoteUploadSpy {
        fn drop(&mut self) {
            let finished = self.server.is_finished();
            self.server.abort();
            if !std::thread::panicking() {
                assert!(
                    !finished,
                    "HTTP spy exited unexpectedly; check its task panic"
                );
            }
        }
    }

    fn uploaded_exit_code(artifact: &Bytes) -> u32 {
        let dir = tempfile::tempdir().unwrap();
        super::super::cache_archive::unarchive_bytes(dir.path(), artifact)
            .unwrap();
        let metadata =
            std::fs::read(dir.path().join(CACHE_OUTPUT_METADATA_FILE)).unwrap();
        let cached: CachedTaskExecution =
            rmp_serde::from_slice(&metadata).unwrap();
        cached.exit_code
    }

    async fn run_one_upload(remote: &RemoteUploadSpy) {
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            )),
        )
        .await
        .unwrap()
        .unwrap();
    }

    fn partitions(cache_dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>, root: &Path) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n == "output") {
                        let rel = path.parent().unwrap().parent().unwrap();
                        out.push(
                            rel.strip_prefix(root)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/"),
                        );
                    } else {
                        walk(&path, out, root);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(cache_dir, &mut out, cache_dir);
        out.sort();
        out
    }

    #[tokio::test]
    async fn only_an_untrusted_run_reads_its_isolated_namespace() {
        let headers = "x-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\n";
        for (scopes, expected) in [
            ("read:artifacts,write:artifacts", vec!["isolated", "shared"]),
            (
                "read:artifacts,write:artifacts,publish:artifacts",
                vec!["shared"],
            ),
            ("read:artifacts", vec!["shared"]),
        ] {
            let remote = RemoteUploadSpy::with_scopes(204, scopes).await;
            *remote.probe_headers.lock().unwrap() = headers.into();
            let temp = fixture(&["project1"]).await;
            let task = task("task", "project1", temp.path());
            remote.cache(temp.path()).get(task.get()).await.unwrap();
            let lines = remote.lines.lock().unwrap();
            let seen: Vec<&str> = lines
                .iter()
                .filter(|l| l.starts_with("GET "))
                .map(|l| {
                    if l.contains("namespace=isolated") {
                        "isolated"
                    } else {
                        "shared"
                    }
                })
                .collect();
            assert_eq!(seen, expected, "{scopes}");
        }
    }

    #[tokio::test]
    async fn the_partition_follows_what_the_credential_may_do() {
        let cases = [
            // (scopes, headers, expected partition prefix)
            (
                "read:artifacts,write:artifacts",
                "x-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\n",
                ".partitions/untrusted/",
            ),
            (
                "read:artifacts,write:artifacts,publish:artifacts",
                "x-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\n",
                ".partitions/trusted/",
            ),
            ("read:artifacts", "", ".partitions/trusted/"),
            // Untrusted, but the server named no principal: a partition no
            // trusted run reads.
            ("read:artifacts,write:artifacts", "", ".partitions/unknown/"),
        ];
        for (scopes, headers, expected) in cases {
            let remote = RemoteUploadSpy::with_scopes(204, scopes).await;
            *remote.probe_headers.lock().unwrap() = headers.into();
            let temp = fixture(&["project1"]).await;
            let cache = remote.cache(temp.path());
            let task = task("task", "project1", temp.path());
            cache
                .cache(&new_cache_info_with_exit_code(
                    Some(&LOGS_CONTENT),
                    *task.get(),
                    0,
                ))
                .await
                .unwrap();
            let found = partitions(&temp.path().join(".omni/cache"));
            assert_eq!(found.len(), 1, "{scopes}: {found:?}");
            assert!(found[0].starts_with(expected), "{scopes}: {found:?}");
            if expected.contains("untrusted") {
                assert!(found[0].ends_with("/ci/pr-9"), "{found:?}");
            }
            // The root layout never gets an entry from a remote-enabled run
            // (only the shared file-hash index lives there).
            assert!(
                !temp
                    .path()
                    .join(".omni/cache")
                    .join(path_safe_bs58("project1"))
                    .join("output")
                    .exists(),
                "{scopes}"
            );
        }
    }

    fn path_safe_bs58(project: &str) -> String {
        bs58::encode(project.as_bytes()).into_string()
    }

    #[tokio::test]
    async fn a_probe_that_gives_no_verdict_uses_the_trusted_partition() {
        let temp = fixture(&["project1"]).await;
        // Nothing listens: the probe fails and is unknown.
        let cache = remote_store(temp.path(), "http://127.0.0.1:9");
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .unwrap();
        let found = partitions(&temp.path().join(".omni/cache-restore"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with(".partitions/trusted/"), "{found:?}");
    }

    #[tokio::test]
    async fn a_remote_enabled_run_never_reads_the_root_layout() {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        // A disabled run writes the plain layout.
        let plain = HybridTaskExecutionCacheStore::new(
            temp.path().join(".omni/cache-restore"),
            temp.path(),
            RemoteConfig::new_disabled(),
        );
        plain
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .unwrap();
        assert!(plain.get(task.get()).await.unwrap().is_some());
        // The remote-enabled store over the same directory sees a miss.
        let enabled = remote_store(temp.path(), "http://127.0.0.1:9");
        assert!(enabled.get(task.get()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn every_view_hashes_inputs_against_the_whole_cache_directory() {
        let remote =
            RemoteUploadSpy::with_scopes(204, "read:artifacts,write:artifacts")
                .await;
        *remote.probe_headers.lock().unwrap() =
            "x-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\n".into();
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .unwrap();
        let views = cache.views().await;
        assert!(views.len() >= 2);
        for view in &views {
            assert_eq!(view.base_dir, cache.base_dir);
            assert!(view.is_view);
        }
        assert!(views.iter().any(|v| v.cache_dir != cache.base_dir));
    }

    #[tokio::test]
    async fn the_local_only_store_keeps_entries_in_the_local_partition() {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let cache_dir = temp.path().join(".omni/cache-restore");
        let local = HybridTaskExecutionCacheStore::new_local_only(
            &cache_dir,
            temp.path(),
        );
        local
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .unwrap();
        assert!(local.get(task.get()).await.unwrap().is_some());
        let found = partitions(&cache_dir);
        assert_eq!(found, [".partitions/local"], "{found:?}");
        // The root layout is untouched, and a remote-enabled run (whose own
        // partition is elsewhere) does not see the entry.
        let enabled = remote_store(temp.path(), "http://127.0.0.1:9");
        assert!(enabled.get(task.get()).await.unwrap().is_none());
        // Stats and prune still cover it.
        let stats = local
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(stats.projects.len(), 1);
    }

    #[tokio::test]
    async fn stats_and_prune_cover_every_partition_without_a_probe() {
        let remote =
            RemoteUploadSpy::with_scopes(204, "read:artifacts,write:artifacts")
                .await;
        *remote.probe_headers.lock().unwrap() =
            "x-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\n".into();
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .unwrap();
        let probes_before = remote.lines.lock().unwrap().len();

        // A store with no remote at all, over the same directory.
        let plain = HybridTaskExecutionCacheStore::new(
            temp.path().join(".omni/cache"),
            temp.path(),
            RemoteConfig::new_disabled(),
        );
        let stats = plain
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(stats.projects.len(), 1);
        assert_eq!(stats.projects[0].tasks.len(), 1);

        let pruned = plain
            .prune_caches(&PruneCacheArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(pruned.len(), 1);
        pruned.iter().for_each(|e| {
            assert!(e.entry_dir.to_string_lossy().contains(".partitions"))
        });
        plain.force_prune_caches(&pruned).await.unwrap();
        let after = plain
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["project1"],
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(after.projects.iter().all(|p| p.tasks.is_empty()));
        assert_eq!(remote.lines.lock().unwrap().len(), probes_before);
    }

    #[tokio::test]
    async fn a_shared_upload_is_signed_and_the_signature_verifies() {
        let key = signer();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("key");
        let kp = omni_remote_cache_client::generate_keypair().unwrap();
        std::fs::write(&file, format!("{}\n", *kp.private_key)).unwrap();
        let mut remote = RemoteUploadSpy::with_scopes(
            204,
            "read:artifacts,write:artifacts,publish:artifacts",
        )
        .await;
        remote.target = RemoteTarget::Shared;
        let temp = fixture(&["project1"]).await;
        let mut cache = remote.cache(temp.path());
        if let RemoteConfig::Enabled(conf) = &mut cache.remote_config {
            conf.signing =
                Box::new(super::super::signing_policy::SigningPolicy {
                    verify: EffectiveVerify::Require,
                    trusted: vec![
                        omni_remote_cache_client::TrustedKey::parse(
                            &kp.id,
                            &kp.public_key,
                        )
                        .unwrap(),
                        omni_remote_cache_client::TrustedKey::parse(
                            &key.id,
                            &key.public_key,
                        )
                        .unwrap(),
                    ],
                    signing_key: Some(
                        omni_configurations::SigningKeyConfiguration {
                            id: kp.id.clone(),
                            private_key_env: None,
                            private_key_file: Some(file),
                        },
                    ),
                });
        }
        let t0 = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *t0.get(),
                0,
            ))
            .await
            .unwrap();
        let requests = remote.requests.lock().unwrap().clone();
        let heads = remote.heads.lock().unwrap().clone();
        let put = heads
            .iter()
            .find(|h| h.starts_with("PUT"))
            .expect("an upload");
        let body = &requests.iter().find(|r| r.0 == "PUT").unwrap().1;
        let signature = put
            .lines()
            .find_map(|l| l.strip_prefix("x-omni-signature: "))
            .expect("a signature header");
        let digest = put
            .split_whitespace()
            .nth(1)
            .unwrap()
            .trim_start_matches("/v1/artifacts/")
            .split('?')
            .next()
            .unwrap();
        // It verifies for exactly this place, digest and archive hash.
        use sha2::{Digest as _, Sha256};
        let hash: [u8; 32] = Sha256::digest(body).into();
        let message = omni_remote_cache_client::signed_message(
            "t", "o", "w", "e", digest, &hash,
        );
        let trusted =
            omni_remote_cache_client::TrustedKey::parse(&kp.id, &kp.public_key)
                .unwrap();
        assert_eq!(
            omni_remote_cache_client::verify(
                &[trusted],
                Some(signature),
                &message
            ),
            Ok(kp.id.clone())
        );
    }

    #[tokio::test]
    async fn a_workspace_that_trusts_keys_gets_no_unsigned_shared_upload() {
        let key = signer();
        let mut remote = RemoteUploadSpy::with_scopes(
            204,
            "read:artifacts,write:artifacts,publish:artifacts",
        )
        .await;
        remote.target = RemoteTarget::Shared;
        let temp = fixture(&["project1"]).await;
        let mut cache = remote.cache(temp.path());
        if let RemoteConfig::Enabled(conf) = &mut cache.remote_config {
            conf.signing =
                Box::new(super::super::signing_policy::SigningPolicy {
                    verify: EffectiveVerify::Require,
                    trusted: vec![
                        omni_remote_cache_client::TrustedKey::parse(
                            &key.id,
                            &key.public_key,
                        )
                        .unwrap(),
                    ],
                    signing_key: None,
                });
        }
        let t0 = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *t0.get(),
                0,
            ))
            .await
            .unwrap();
        assert!(remote.requests.lock().unwrap().is_empty());
        // The same machine can still publish unsigned when nothing is trusted.
        let remote = RemoteUploadSpy::with_scopes(
            204,
            "read:artifacts,write:artifacts,publish:artifacts",
        )
        .await;
        let mut remote = remote;
        remote.target = RemoteTarget::Shared;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let t0 = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *t0.get(),
                0,
            ))
            .await
            .unwrap();
        let heads = remote.heads.lock().unwrap();
        let put = heads
            .iter()
            .find(|h| h.starts_with("PUT"))
            .expect("an upload");
        assert!(!put.to_ascii_lowercase().contains("x-omni-signature"));
    }

    #[tokio::test]
    async fn a_run_under_require_discards_an_entry_no_trusted_key_verified() {
        let temp = fixture(&["project1"]).await;
        let t0 = task("task", "project1", temp.path());
        let archive = genuine_archive(&temp, t0.get()).await;
        let digest = digest_of(&temp, t0.get()).await;
        let good = signer();
        let headers = Arc::new(std::sync::Mutex::new(signed_headers(
            &good, &archive, &digest, "e",
        )));
        let (url, server) = serve_with_live_headers(
            archive.clone(),
            "read:artifacts",
            headers.clone(),
        )
        .await;
        let store =
            verifying_store(&temp, &url, &[&good], EffectiveVerify::Require);
        store.get(t0.get()).await.unwrap().unwrap();
        let found = markers(&store.cache_dir);
        assert_eq!(found.len(), 1);
        let marker = store.cache_dir.join(&found[0].0);

        // While a trusted key vouches for it, the local entry is reused even
        // if the server would now refuse to serve it.
        *headers.lock().unwrap() = String::new();
        let reuse =
            verifying_store(&temp, &url, &[&good], EffectiveVerify::Require);
        assert!(reuse.get(t0.get()).await.unwrap().is_some());
        assert!(marker.exists());

        // Restored under a weaker policy (`none`), it is not reusable under
        // `require`: it is discarded, and the unsigned refetch is a miss.
        std::fs::write(&marker, "v1 origin=shared verified_by=none\n").unwrap();
        let strict =
            verifying_store(&temp, &url, &[&good], EffectiveVerify::Require);
        assert!(strict.get(t0.get()).await.unwrap().is_none());
        assert!(!marker.exists(), "the unverified entry was kept");

        // A key that is not trusted today does not vouch either.
        let other = signer();
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(
            &marker,
            format!("v1 origin=shared verified_by={}\n", other.id),
        )
        .unwrap();
        // (The metadata file is gone with the discarded entry, so this only
        // exercises the marker rule: the directory is removed.)
        let again =
            verifying_store(&temp, &url, &[&good], EffectiveVerify::Require);
        let _ = again.get(t0.get()).await;
        assert!(
            !marker.exists(),
            "an entry vouched for by an untrusted key was kept"
        );
        server.abort();
    }

    #[tokio::test]
    async fn an_untrusted_run_still_verifies_the_shared_fallback() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let temp = fixture(&["project1"]).await;
        let t0 = task("task", "project1", temp.path());
        let archive = genuine_archive(&temp, t0.get()).await;
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        let body = archive.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(socket.read_u8().await.unwrap());
                }
                let head = String::from_utf8(head).unwrap();
                let line = head.lines().next().unwrap().to_string();
                log.lock().unwrap().push(line.clone());
                let response = if line.starts_with("HEAD /v1/artifacts?") {
                    b"HTTP/1.1 204 No Content\r\nx-omni-scopes: read:artifacts,write:artifacts\r\nx-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\nConnection: close\r\n\r\n".to_vec()
                } else if line.starts_with("GET ")
                    && line.contains("namespace=isolated")
                {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                } else {
                    // The shared entry: valid archive, but unsigned.
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nx-omni-content-sha256: {}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        hex_of(&body)
                    )
                    .into_bytes();
                    r.extend_from_slice(&body);
                    r
                };
                socket.write_all(&response).await.unwrap();
            }
        });
        let trusted = signer();
        let store =
            verifying_store(&temp, &url, &[&trusted], EffectiveVerify::Require);
        assert!(store.get(t0.get()).await.unwrap().is_none());
        server.abort();
        let lines = seen.lock().unwrap().clone();
        assert!(
            lines.iter().any(|l| l.contains("namespace=isolated")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(
                |l| l.starts_with("GET ") && l.contains("namespace=shared")
            ),
            "{lines:?}"
        );
        assert!(
            markers(&store.cache_dir).is_empty(),
            "an unsigned shared entry was restored"
        );
    }

    #[tokio::test]
    async fn the_policy_selects_the_local_partition() {
        let temp = fixture(&["project1"]).await;
        let (url, server) =
            serve_with_headers(vec![], "read:artifacts", String::new()).await;
        let (a, b) = (signer(), signer());
        let dir = |trusted: &[&Signer], mode| {
            let store = verifying_store(&temp, &url, trusted, mode);
            async move { store.local_partition().await.dir.clone() }
        };
        let base = dir(&[&a], EffectiveVerify::Require).await;
        assert_eq!(base, dir(&[&a], EffectiveVerify::Require).await);
        for other in [
            dir(&[&a, &b], EffectiveVerify::Require).await,
            dir(&[&b], EffectiveVerify::Require).await,
            dir(&[], EffectiveVerify::Require).await,
            dir(&[&a], EffectiveVerify::Warn).await,
            dir(&[&a], EffectiveVerify::Off).await,
        ] {
            assert_ne!(base, other);
        }
        server.abort();
    }

    #[tokio::test]
    async fn nothing_is_sent_when_push_is_off() {
        let mut remote = RemoteUploadSpy::new(204).await;
        remote.push = false;
        run_one_upload(&remote).await;
        assert!(remote.requests.lock().unwrap().is_empty());
        let lines = remote.lines.lock().unwrap();
        assert!(
            lines.iter().all(|l| l.starts_with("HEAD /v1/artifacts?")),
            "only the probe, which picks the local partition: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_read_only_credential_makes_no_upload_attempt() {
        let remote = RemoteUploadSpy::with_scopes(204, "read:artifacts").await;
        run_one_upload(&remote).await;
        assert!(remote.requests.lock().unwrap().is_empty());
        assert_eq!(remote.lines.lock().unwrap().len(), 1, "only the probe");
    }

    #[tokio::test]
    async fn a_probe_that_is_not_a_verdict_does_not_stop_uploads() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // A server that answers the probe with a 500 and stores everything.
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let header = String::from_utf8(header).unwrap();
                let line = header.lines().next().unwrap().to_string();
                if line.starts_with("PUT") {
                    let length = header
                        .lines()
                        .find_map(|l| {
                            let (n, v) = l.split_once(':')?;
                            n.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).await.unwrap();
                }
                let status = if line.starts_with("PUT") { 204 } else { 500 };
                log.lock().unwrap().push(line);
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let temp = fixture(&["project1"]).await;
        let mut cache = remote_store(temp.path(), &url);
        if let RemoteConfig::Enabled(conf) = &mut cache.remote_config {
            conf.push = true;
        }
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .unwrap();
        server.abort();
        assert!(
            seen.lock().unwrap().iter().any(|l| l.starts_with("PUT")),
            "a 500 probe is unknown, not a refusal"
        );
    }

    #[tokio::test]
    async fn the_shared_target_needs_publish_and_goes_to_shared() {
        let mut remote =
            RemoteUploadSpy::with_scopes(204, "read:artifacts,write:artifacts")
                .await;
        remote.target = RemoteTarget::Shared;
        run_one_upload(&remote).await;
        assert!(
            remote.requests.lock().unwrap().is_empty(),
            "write alone cannot publish"
        );

        let mut remote = RemoteUploadSpy::with_scopes(
            204,
            "read:artifacts,write:artifacts,publish:artifacts",
        )
        .await;
        remote.target = RemoteTarget::Shared;
        run_one_upload(&remote).await;
        let lines = remote.lines.lock().unwrap();
        let put = lines.iter().find(|l| l.starts_with("PUT")).unwrap();
        assert!(put.contains("target=shared"), "{put}");
        assert!(!put.contains("scope="), "{put}");
    }

    #[tokio::test]
    async fn an_isolated_upload_names_its_scope_and_the_probe_runs_once() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let tasks = [
            task("a", "project1", temp.path()),
            task("b", "project1", temp.path()),
        ];
        let logs = LOGS_CONTENT;
        let infos = [
            new_cache_info_with_exit_code(Some(&logs), *tasks[0].get(), 0),
            new_cache_info_with_exit_code(Some(&logs), *tasks[1].get(), 0),
        ];
        cache.cache_many(&infos).await.unwrap();
        let lines = remote.lines.lock().unwrap();
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("HEAD")).count(),
            1,
            "{lines:?}"
        );
        let puts: Vec<_> =
            lines.iter().filter(|l| l.starts_with("PUT")).collect();
        assert_eq!(puts.len(), 2);
        assert!(
            puts.iter()
                .all(|l| l.contains("target=isolated")
                    && l.contains("&scope=pr-1")),
            "{puts:?}"
        );
    }

    #[tokio::test]
    async fn an_upload_over_the_size_cap_is_skipped() {
        let mut remote = RemoteUploadSpy::new(204).await;
        remote.push = true;
        let temp = fixture(&["project1"]).await;
        let mut cache = remote.cache(temp.path());
        if let RemoteConfig::Enabled(conf) = &mut cache.remote_config {
            conf.max_artifact_size = Some(16);
        }
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .unwrap();
        assert!(remote.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn the_server_can_only_lower_the_size_cap() {
        let mut conf = EnabledRemoteConfig::default();
        let probe = |n| RemoteProbe {
            server_max_artifact_size: Some(n),
            ..Default::default()
        };
        assert_eq!(conf.size_cap(None), DEFAULT_MAX_ARTIFACT_SIZE);
        assert_eq!(conf.size_cap(Some(&probe(1024))), 1024);
        assert_eq!(
            conf.size_cap(Some(&probe(u64::MAX))),
            DEFAULT_MAX_ARTIFACT_SIZE,
            "a huge advertised value never raises the default"
        );
        conf.max_artifact_size = Some(100);
        assert_eq!(conf.size_cap(Some(&probe(1 << 40))), 100);
        assert_eq!(conf.size_cap(Some(&probe(10))), 10);
    }

    #[tokio::test]
    async fn failed_execution_stays_local_without_remote_calls() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            )),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(remote.requests.lock().unwrap().is_empty());
        // The local partition depends on what the credential may do, so the
        // probe is the one request even a failed task causes.
        {
            let lines = remote.lines.lock().unwrap();
            assert!(
                lines.iter().all(|l| l.starts_with("HEAD /v1/artifacts?")),
                "{lines:?}"
            );
        }
        assert_eq!(cache.get(task.get()).await.unwrap().unwrap().exit_code, 1);
    }

    #[tokio::test]
    async fn mixed_batch_uploads_only_successful_metadata() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let success = task("success", "project1", temp.path());
        let failure = task("failure", "project1", temp.path());
        let logs = LOGS_CONTENT;
        let infos = [
            new_cache_info_with_exit_code(Some(&logs), *success.get(), 0),
            new_cache_info_with_exit_code(Some(&logs), *failure.get(), 1),
        ];
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache_many(&infos),
        )
        .await
        .unwrap()
        .unwrap();
        let requests = remote.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "PUT");
        assert_eq!(uploaded_exit_code(&requests[0].1), 0);
    }

    #[tokio::test]
    async fn remote_rejection_does_not_fail_local_failure_to_success_upgrade() {
        let remote = RemoteUploadSpy::new(503).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            ))
            .await
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get())),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(cache.get(task.get()).await.unwrap().unwrap().exit_code, 0);
        let requests = remote.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "PUT");
        assert_eq!(uploaded_exit_code(&requests[0].1), 0);
    }

    #[tokio::test]
    async fn cached_success_can_upload_but_a_later_failure_never_does() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        for exit_code in [0, 0, 1] {
            tokio::time::timeout(
                std::time::Duration::from_secs(15),
                cache.cache(&new_cache_info_with_exit_code(
                    Some(&LOGS_CONTENT),
                    *task.get(),
                    exit_code,
                )),
            )
            .await
            .unwrap()
            .unwrap();
        }
        assert_eq!(cache.get(task.get()).await.unwrap().unwrap().exit_code, 0);
        let requests = remote.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (method, archive) in requests.iter() {
            assert_eq!(method, "PUT");
            assert_eq!(uploaded_exit_code(archive), 0);
        }
    }

    #[tokio::test]
    async fn remote_transport_failure_retains_a_successful_local_result() {
        let remote = RemoteUploadSpy::new(0).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get())),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(cache.get(task.get()).await.unwrap().unwrap().exit_code, 0);
        assert!(!remote.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn remote_conflict_is_accepted_without_retrying() {
        let remote = RemoteUploadSpy::new(409).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get())),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(cache.get(task.get()).await.unwrap().unwrap().exit_code, 0);
        let requests = remote.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "409 is final: no retry and no HEAD");
        assert_eq!(requests[0].0, "PUT");
    }

    #[tokio::test]
    async fn a_write_only_key_uploads_without_any_read_request() {
        // Immutability is enforced by the server's create-only write, so the
        // client never needs `read:artifacts` (HEAD) just to publish.
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cache.cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get())),
        )
        .await
        .unwrap()
        .unwrap();
        let requests = remote.requests.lock().unwrap();
        assert!(requests.iter().all(|(method, _)| method == "PUT"));
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn successful_caller_cannot_upload_a_failed_publication_winner() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let task = task("task", "project1", temp.path());
        // Model the persisted winner of a first-publication race. The losing
        // successful publisher still receives Ok from publish_staged_dir, but
        // it must not queue the winning failed entry for remote upload.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            ))
            .await
            .unwrap();
        let collected = cache
            .collect(
                &[*task.get()],
                &CollectConfig {
                    cache_output_dirs: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let metadata = collected[0]
            .cache_output_dir
            .as_ref()
            .unwrap()
            .join(CACHE_OUTPUT_METADATA_FILE);
        let output_dir = metadata.parent().unwrap();
        let staging = staging_dir_for(output_dir);
        tokio::fs::create_dir_all(&staging).await.unwrap();
        let mut successful: CachedTaskExecution =
            rmp_serde::from_slice(&tokio::fs::read(&metadata).await.unwrap())
                .unwrap();
        successful.exit_code = 0;
        tokio::fs::write(
            staging.join(CACHE_OUTPUT_METADATA_FILE),
            rmp_serde::to_vec(&successful).unwrap(),
        )
        .await
        .unwrap();
        publish_staged_dir(&staging, output_dir, &metadata, false)
            .await
            .unwrap();
        assert_eq!(
            cache.read_cached_exit_code(&metadata).await.unwrap(),
            Some(1)
        );
        // This is a unit-level publication/gating test, not a scheduler-driven
        // race through cache_many. The upload queue uses this same helper.
        assert!(!cache.can_upload_cached_result(0, &metadata).await);
        assert!(
            !cache
                .can_upload_cached_result(0, &temp.path().join("missing"))
                .await
        );
        assert!(remote.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unreadable_persisted_outcome_skips_remote_upload_without_error() {
        let remote = RemoteUploadSpy::new(204).await;
        let temp = fixture(&["project1"]).await;
        let cache = remote.cache(temp.path());
        let metadata = temp.path().join("corrupt-metadata");
        tokio::fs::write(&metadata, b"not messagepack metadata")
            .await
            .unwrap();
        assert!(!cache.can_upload_cached_result(0, &metadata).await);
        assert!(remote.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn caches_outputs_on_a_different_device() {
        use std::os::unix::fs::MetadataExt;

        let temp = fixture(&["project1"]).await;
        // /dev/shm provides a second filesystem on typical Linux hosts. Other
        // hosts still exercise the error-kind policy in omni_utils's tests.
        let Ok(cache_dir) = tempfile::tempdir_in("/dev/shm") else {
            return;
        };
        if std::fs::metadata(temp.path()).unwrap().dev()
            == std::fs::metadata(cache_dir.path()).unwrap().dev()
        {
            return;
        }
        let cache = HybridTaskExecutionCacheStore::new(
            cache_dir.path(),
            temp.path(),
            RemoteConfig::new_disabled(),
        );
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .unwrap();
        std::fs::remove_dir_all(temp.path().join("project1/dist")).unwrap();
        let cached = cache.get(task.get()).await.unwrap().unwrap();
        assert_eq!(cached.files.len(), 2);
        for file in cached.files {
            assert_eq!(
                std::fs::read_to_string(file.cached_path).unwrap(),
                JS_CONTENT
            );
        }
    }

    #[tokio::test]
    async fn test_cache_unchanged() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);

        let task = task("task", "project1", dir);
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .expect("failed to cache");

        let cached_output2 = cache
            .get(task.get())
            .await
            .expect("failed to get cached output");

        assert!(cached_output1.is_none(), "cached output should not exist");
        assert!(cached_output2.is_some(), "cached output should exist");
    }

    async fn rewrite_entry_metadata(
        cache: &HybridTaskExecutionCacheStore,
        task: &TaskExecutionInfo<'_>,
        edit: impl FnOnce(&mut CachedTaskExecution),
    ) {
        let collected = cache
            .collect(
                &[*task],
                &CollectConfig {
                    cache_output_dirs: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let metadata = collected[0]
            .cache_output_dir
            .as_ref()
            .unwrap()
            .join(CACHE_OUTPUT_METADATA_FILE);
        let mut entry: CachedTaskExecution =
            rmp_serde::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        edit(&mut entry);
        std::fs::write(&metadata, rmp_serde::to_vec(&entry).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn an_entry_with_an_escaping_path_is_a_miss() {
        use omni_types::{OmniPath, Root};

        type Edit = Box<dyn Fn(&mut CachedTaskExecution)>;
        let hostile: Vec<(&str, Edit)> = vec![
            (
                "original_path with ..",
                Box::new(|e| {
                    e.files[0].original_path =
                        OmniPath::new_rooted("../../escape.js", Root::Project)
                }),
            ),
            (
                "original_path hidden .. segment",
                Box::new(|e| {
                    e.files[0].original_path = OmniPath::new_rooted(
                        "dist/../../escape.js",
                        Root::Workspace,
                    )
                }),
            ),
            (
                "original_path absolute under a root",
                Box::new(|e| {
                    e.files[0].original_path =
                        OmniPath::new_rooted("/tmp/escape.js", Root::Project)
                }),
            ),
            (
                "original_path unrooted absolute",
                Box::new(|e| {
                    e.files[0].original_path = OmniPath::new("/tmp/escape.js")
                }),
            ),
            (
                "original_path empty",
                Box::new(|e| {
                    e.files[0].original_path =
                        OmniPath::new_rooted("", Root::Project)
                }),
            ),
            (
                "cached_path with ..",
                Box::new(|e| {
                    e.files[0].cached_path =
                        PathBuf::from("../../../etc/passwd")
                }),
            ),
            (
                "cached_path absolute",
                Box::new(|e| {
                    e.files[0].cached_path = PathBuf::from("/etc/passwd")
                }),
            ),
            (
                "logs_path absolute",
                Box::new(|e| e.logs_path = Some(PathBuf::from("/etc/passwd"))),
            ),
            (
                "logs_path with ..",
                Box::new(|e| e.logs_path = Some(PathBuf::from("../../x"))),
            ),
        ];

        for (name, edit) in hostile {
            let temp = fixture(&["project1"]).await;
            let cache = cache_store(temp.path());
            let task = task("task", "project1", temp.path());
            cache
                .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
                .await
                .unwrap();
            assert!(
                cache.get(task.get()).await.unwrap().is_some(),
                "control: the untouched entry must hit ({name})"
            );

            rewrite_entry_metadata(&cache, task.get(), |e| edit(e)).await;

            assert!(
                cache.get(task.get()).await.unwrap().is_none(),
                "an entry with {name} must be a miss"
            );
        }
    }

    /// Serves `body` for every GET and 404 for anything else.
    async fn serve_remote_entry(
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let response = if header.starts_with(b"GET ") {
                    let mut head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    head.extend_from_slice(&body);
                    head
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                };
                socket.write_all(&response).await.unwrap();
            }
        });
        (url, server)
    }

    fn remote_store(root: &Path, url: &str) -> HybridTaskExecutionCacheStore {
        HybridTaskExecutionCacheStore::new(
            root.join(".omni/cache-restore"),
            root,
            RemoteConfig::Enabled(EnabledRemoteConfig {
                api_base_url: url.to_string(),
                api_key: "k".to_string(),
                tenant_code: "t".to_string(),
                organization_code: "o".to_string(),
                workspace_code: "w".to_string(),
                environment_code: Some("e".to_string()),
                push: false,
                target: RemoteTarget::Isolated,
                scope: ScopeConfig::Auto,
                max_artifact_size: None,
                signing: Default::default(),
            }),
        )
    }

    /// The archive of a real entry for `task`, as a producer would upload it.
    async fn genuine_archive(
        temp: &tempfile::TempDir,
        info: &TaskExecutionInfo<'_>,
    ) -> Vec<u8> {
        let producer = cache_store(temp.path());
        producer
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *info))
            .await
            .unwrap();
        let collected = producer
            .collect(
                &[*info],
                &CollectConfig {
                    cache_output_dirs: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut out = Vec::new();
        archive(
            collected[0].cache_output_dir.as_ref().unwrap(),
            Cursor::new(&mut out),
        )
        .unwrap();
        out
    }

    /// Serves `body` to every `GET`, and answers the access probe with
    /// `scopes` and a principal and scope, so the run is untrusted when
    /// `scopes` has `write` and no `publish`.
    async fn serve_with_probe(
        body: Vec<u8>,
        scopes: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let response = if header.starts_with(b"HEAD /v1/artifacts?") {
                    format!("HTTP/1.1 204 No Content\r\nx-omni-scopes: {scopes}\r\nx-omni-principal-id: ci\r\nx-omni-scope: pr-9\r\nConnection: close\r\n\r\n").into_bytes()
                } else if header.starts_with(b"GET ") {
                    let mut head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    head.extend_from_slice(&body);
                    head
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                };
                socket.write_all(&response).await.unwrap();
            }
        });
        (url, server)
    }

    /// Like [`serve_with_probe`], with `extra` response headers (each ending in
    /// CRLF) on every `GET`.
    async fn serve_with_headers(
        body: Vec<u8>,
        scopes: &'static str,
        extra: String,
    ) -> (String, tokio::task::JoinHandle<()>) {
        serve_with_live_headers(
            body,
            scopes,
            Arc::new(std::sync::Mutex::new(extra)),
        )
        .await
    }

    /// As [`serve_with_headers`], with headers a test can change while the
    /// server runs.
    async fn serve_with_live_headers(
        body: Vec<u8>,
        scopes: &'static str,
        extra: Arc<std::sync::Mutex<String>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let response = if header.starts_with(b"HEAD /v1/artifacts?") {
                    format!("HTTP/1.1 204 No Content\r\nx-omni-scopes: {scopes}\r\nConnection: close\r\n\r\n").into_bytes()
                } else if header.starts_with(b"GET ") {
                    let extra = extra.lock().unwrap().clone();
                    let mut head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    head.extend_from_slice(&body);
                    head
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                };
                socket.write_all(&response).await.unwrap();
            }
        });
        (url, server)
    }

    fn hex_of(bytes: &[u8]) -> String {
        use sha2::{Digest as _, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// The digest a restore asks for, as it appears in URLs and signatures.
    async fn digest_of(
        temp: &tempfile::TempDir,
        info: &TaskExecutionInfo<'_>,
    ) -> String {
        let store = cache_store(temp.path());
        let collected = store
            .collect(
                &[*info],
                &CollectConfig {
                    digests: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        bs58::encode(collected[0].digest.unwrap()).into_string()
    }

    struct Signer {
        handle: omni_remote_cache_client::SigningHandle,
        public_key: String,
        id: String,
    }

    fn signer() -> Signer {
        let kp = omni_remote_cache_client::generate_keypair().unwrap();
        Signer {
            handle: omni_remote_cache_client::SigningHandle::from_base64(
                &kp.private_key,
            )
            .unwrap(),
            public_key: kp.public_key,
            id: kp.id,
        }
    }

    fn signed_headers(
        signer: &Signer,
        archive: &[u8],
        digest: &str,
        env: &str,
    ) -> String {
        use sha2::{Digest as _, Sha256};
        let hash: [u8; 32] = Sha256::digest(archive).into();
        let message = omni_remote_cache_client::signed_message(
            "t", "o", "w", env, digest, &hash,
        );
        format!(
            "x-omni-content-sha256: {}\r\nx-omni-signature: {}\r\n",
            hex_of(archive),
            signer.handle.sign(&message)
        )
    }

    fn verifying_store(
        temp: &tempfile::TempDir,
        url: &str,
        trusted: &[&Signer],
        mode: EffectiveVerify,
    ) -> HybridTaskExecutionCacheStore {
        let mut store = remote_store(temp.path(), url);
        if let RemoteConfig::Enabled(conf) = &mut store.remote_config {
            conf.signing =
                Box::new(super::super::signing_policy::SigningPolicy {
                    verify: mode,
                    trusted: trusted
                        .iter()
                        .map(|s| {
                            omni_remote_cache_client::TrustedKey::parse(
                                &s.id,
                                &s.public_key,
                            )
                            .unwrap()
                        })
                        .collect(),
                    signing_key: None,
                });
        }
        store
    }

    #[tokio::test]
    async fn require_restores_only_entries_signed_by_a_trusted_key() {
        let temp = fixture(&["project1"]).await;
        let t0 = task("task", "project1", temp.path());
        let archive = genuine_archive(&temp, t0.get()).await;
        let digest = digest_of(&temp, t0.get()).await;
        let good = signer();
        let stranger = signer();

        // Signed by the trusted key: restored, and the marker names the key.
        let headers = signed_headers(&good, &archive, &digest, "e");
        let (url, server) = serve_with_headers(
            archive.clone(),
            "read:artifacts",
            headers.clone(),
        )
        .await;
        let store =
            verifying_store(&temp, &url, &[&good], EffectiveVerify::Require);
        assert!(store.get(t0.get()).await.unwrap().is_some());
        server.abort();
        let found = markers(&store.cache_dir);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].1,
            format!("v1 origin=shared verified_by={}\n", good.id)
        );

        // Each of these must be a miss that extracts nothing.
        let other_digest =
            signed_headers(&good, &archive, "someotherdigest", "e");
        let other_env = signed_headers(&good, &archive, &digest, "e2");
        let by_stranger = signed_headers(&stranger, &archive, &digest, "e");
        let tampered_bytes =
            signed_headers(&good, b"different archive", &digest, "e");
        // A correct hash header for the served bytes, but a signature made over
        // the hash of other bytes: only the signature check can catch it.
        let mismatched_signature = {
            let sig_line = tampered_bytes.lines().nth(1).unwrap();
            format!(
                "x-omni-content-sha256: {}\r\n{sig_line}\r\n",
                hex_of(&archive)
            )
        };
        let mut no_signature = headers.clone();
        no_signature = no_signature
            .lines()
            .next()
            .map(|l| format!("{l}\r\n"))
            .unwrap();
        for (name, header) in [
            ("replayed under another digest", other_digest),
            ("replayed under another environment", other_env),
            ("signed by an untrusted key", by_stranger),
            (
                "hash header and signature cover other bytes",
                tampered_bytes,
            ),
            (
                "signature covers other bytes than the hash header",
                mismatched_signature,
            ),
            ("unsigned", no_signature),
            ("no provenance at all (fails the hash check)", String::new()),
        ] {
            let fresh = fixture(&["project1"]).await;
            let ft = task("task", "project1", fresh.path());
            let (url, server) =
                serve_with_headers(archive.clone(), "read:artifacts", header)
                    .await;
            let store = verifying_store(
                &fresh,
                &url,
                &[&good],
                EffectiveVerify::Require,
            );
            assert!(store.get(ft.get()).await.unwrap().is_none(), "{name}");
            server.abort();
            assert!(
                markers(&store.cache_dir).is_empty(),
                "{name}: extracted something"
            );
        }
    }

    #[tokio::test]
    async fn warn_restores_with_no_key_recorded_and_off_does_not_check() {
        let temp = fixture(&["project1"]).await;
        let t0 = task("task", "project1", temp.path());
        let archive = genuine_archive(&temp, t0.get()).await;
        for mode in [EffectiveVerify::Warn, EffectiveVerify::Off] {
            let fresh = fixture(&["project1"]).await;
            let ft = task("task", "project1", fresh.path());
            let (url, server) = serve_with_headers(
                archive.clone(),
                "read:artifacts",
                String::new(),
            )
            .await;
            let store = verifying_store(&fresh, &url, &[], mode);
            assert!(store.get(ft.get()).await.unwrap().is_some(), "{mode:?}");
            server.abort();
            let found = markers(&store.cache_dir);
            assert_eq!(
                found[0].1, "v1 origin=shared verified_by=none\n",
                "{mode:?}"
            );
        }
    }

    /// Every origin marker under `dir`, with the partition it is in.
    fn markers(dir: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .file_name()
                    .is_some_and(|n| n == ".omni-origin-remote")
                {
                    out.push((
                        path.strip_prefix(dir)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                        std::fs::read_to_string(&path).unwrap(),
                    ));
                }
            }
        }
        out
    }

    #[tokio::test]
    async fn a_trusted_restore_is_marked_shared_in_the_trusted_partition() {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let body = genuine_archive(&temp, task.get()).await;
        let (url, server) = serve_with_probe(body, "read:artifacts").await;
        let store = remote_store(temp.path(), &url);
        assert!(store.get(task.get()).await.unwrap().is_some());
        server.abort();
        let found = markers(&temp.path().join(".omni/cache-restore"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.starts_with(".partitions/trusted/"), "{found:?}");
        assert_eq!(found[0].1, "v1 origin=shared verified_by=none\n");
    }

    #[tokio::test]
    async fn an_untrusted_restore_is_marked_isolated_in_its_own_partition() {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let body = genuine_archive(&temp, task.get()).await;
        let (url, server) =
            serve_with_probe(body, "read:artifacts,write:artifacts").await;
        let mut store = remote_store(temp.path(), &url);
        if let RemoteConfig::Enabled(conf) = &mut store.remote_config {
            conf.scope = ScopeConfig::Literal("pr-9".into());
        }
        assert!(store.get(task.get()).await.unwrap().is_some());
        server.abort();
        let found = markers(&temp.path().join(".omni/cache-restore"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].0.starts_with(".partitions/untrusted/"),
            "{found:?}"
        );
        assert!(found[0].0.contains("/ci/pr-9/"), "{found:?}");
        assert_eq!(found[0].1, "v1 origin=isolated verified_by=none\n");
    }

    #[tokio::test]
    async fn a_trusted_run_discards_an_entry_marked_isolated_and_refetches_it()
    {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let body = genuine_archive(&temp, task.get()).await;
        let (url, server) = serve_with_probe(body, "read:artifacts").await;
        let store = remote_store(temp.path(), &url);
        store.get(task.get()).await.unwrap().unwrap();
        let cache_dir = temp.path().join(".omni/cache-restore");
        let marker = {
            let found = markers(&cache_dir);
            cache_dir.join(&found[0].0)
        };
        std::fs::write(&marker, "v1 origin=isolated verified_by=none\n")
            .unwrap();
        let fresh = remote_store(temp.path(), &url);
        fresh.get(task.get()).await.unwrap().unwrap();
        server.abort();
        let found = markers(&cache_dir);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1, "v1 origin=shared verified_by=none\n");
    }

    #[tokio::test]
    async fn a_corrupt_remote_archive_leaves_no_entry_and_no_staging_directory()
    {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let (url, server) =
            serve_with_probe(b"not an archive".to_vec(), "read:artifacts")
                .await;
        let store = remote_store(temp.path(), &url);
        assert!(store.get(task.get()).await.unwrap().is_none());
        server.abort();
        let cache_dir = temp.path().join(".omni/cache-restore");
        assert!(markers(&cache_dir).is_empty());
        let mut stack = vec![cache_dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                assert!(
                    !name.starts_with(STAGING_PREFIX),
                    "leftover staging {name}"
                );
                if entry.path().is_dir() {
                    stack.push(entry.path());
                }
            }
        }
    }

    #[tokio::test]
    async fn an_oversized_remote_entry_is_a_miss_and_does_not_fail_the_run() {
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let (url, server) = serve_remote_entry(vec![7; 100]).await;
        let mut store = remote_store(temp.path(), &url);
        if let RemoteConfig::Enabled(conf) = &mut store.remote_config {
            conf.max_artifact_size = Some(10);
        }
        let got = store.get(task.get()).await.unwrap();
        server.abort();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn a_failing_remote_read_is_a_miss_and_does_not_fail_the_run() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                socket
                    .write_all(b"HTTP/1.1 500 Oops\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }
        });
        let temp = fixture(&["project1"]).await;
        let task = task("task", "project1", temp.path());
        let got = remote_store(temp.path(), &url)
            .get(task.get())
            .await
            .unwrap();
        server.abort();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn a_remote_archive_with_a_link_is_a_miss_and_writes_nothing() {
        let temp = fixture(&["project1"]).await;
        let genuine_task = task("task", "project1", temp.path());

        // Control: a genuine archive of a real entry restores from the remote.
        let producer = cache_store(temp.path());
        producer
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *genuine_task.get()))
            .await
            .unwrap();
        let collected = producer
            .collect(
                &[*genuine_task.get()],
                &CollectConfig {
                    cache_output_dirs: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut genuine = Vec::new();
        archive(
            collected[0].cache_output_dir.as_ref().unwrap(),
            Cursor::new(&mut genuine),
        )
        .unwrap();
        let (url, server) = serve_remote_entry(genuine).await;
        let restored = remote_store(temp.path(), &url)
            .get(genuine_task.get())
            .await
            .unwrap();
        assert!(restored.is_some(), "control: a genuine archive restores");
        server.abort();

        // Hostile: a symlink entry pointing at a directory outside the cache.
        let canary = tempfile::tempdir().unwrap();
        let mut hostile = Vec::new();
        {
            let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
                &mut hostile,
                flate2::Compression::default(),
            ));
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            tar.append_link(&mut header, "output.js.cache", canary.path())
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let (url, server) = serve_remote_entry(hostile).await;
        let victim = fixture(&["project1"]).await;
        let victim_task = task("task", "project1", victim.path());
        let got = remote_store(victim.path(), &url)
            .get(victim_task.get())
            .await
            .unwrap();
        server.abort();
        assert!(got.is_none(), "a link in a remote archive must be a miss");
        assert_eq!(
            std::fs::read_dir(canary.path()).unwrap().count(),
            0,
            "nothing may be written through the link"
        );
    }

    #[tokio::test]
    async fn a_confined_entry_still_restores_with_the_normal_roots() {
        let temp = fixture(&["project1"]).await;
        let cache = cache_store(temp.path());
        let task = task("task", "project1", temp.path());
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .unwrap();
        let entry = cache.get(task.get()).await.unwrap().unwrap();
        assert_eq!(entry.files.len(), 2);
        for file in &entry.files {
            let restored = file.original_path.path().expect("resolved");
            assert!(
                restored.starts_with(temp.path().join("project1")),
                "{restored:?} must stay inside the project"
            );
        }
    }

    #[tokio::test]
    async fn test_success_replaces_cached_failure() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        // A flaky task fails first and its failure is cached.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            ))
            .await
            .expect("failed to cache failure");

        let cached_failure = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached failure should exist");
        assert_eq!(
            cached_failure.exit_code, 1,
            "cached entry should record the failure"
        );

        // A later (forced) re-execution succeeds; it must replace the failure
        // even though it resolves to the same content-addressed digest.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .expect("failed to cache success");

        let cached_success = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached success should exist");
        assert_eq!(
            cached_success.exit_code, 0,
            "successful execution should supersede the cached failure"
        );
    }

    #[tokio::test]
    async fn test_failure_does_not_replace_cached_success() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        // A successful execution is cached first.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .expect("failed to cache success");

        // A later failure of the same digest must not overwrite the success.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            ))
            .await
            .expect("failed to cache failure");

        let cached = cache
            .get(task.get())
            .await
            .expect("failed to get cached output")
            .expect("cached success should still exist");
        assert_eq!(
            cached.exit_code, 0,
            "a cached success must never be downgraded to a failure"
        );
    }

    /// Returns every path under `root` relative to it, sorted, with `/`
    /// separators.
    fn list_tree(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).expect("read_dir") {
                let p = e.expect("entry").path();
                let rel = p.strip_prefix(root).expect("prefix");
                out.push(rel.to_string_lossy().replace('\\', "/"));
                if p.is_dir() {
                    walk(&p, root, out);
                }
            }
        }
        let mut out = vec![];
        walk(root, root, &mut out);
        out.sort();
        out
    }

    /// Pins the on-disk layout of a task cache entry. A change here is a
    /// cache format change and must be deliberate.
    #[tokio::test]
    async fn test_golden_on_disk_layout() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .expect("failed to cache");
        let cached = cache.get(task.get()).await.expect("get").expect("cached");

        let cache_dir = dir.join(".omni/cache");
        let pdir = path_safe("project1");
        let entry = format!(
            "{pdir}/output/{}",
            bs58::encode(&cached.digest).into_string()
        );

        let mut expected = vec![
            ".locks".to_string(),
            ".locks/prune.lock".to_string(),
            pdir.clone(),
            format!("{pdir}/indices"),
            format!("{pdir}/indices/<h>"),
            format!("{pdir}/indices/<h>/partial-hashes.bin"),
            format!("{pdir}/output"),
            entry.clone(),
            format!("{entry}/cache.meta.bin"),
            format!("{entry}/logs.cache"),
        ];
        for f in &cached.files {
            let name = f.cached_path.file_name().expect("name");
            let name = name.to_string_lossy();
            let stem = name.strip_suffix(".cache").expect(".cache suffix");
            assert!(bs58::decode(stem).into_vec().is_ok(), "bs58 stem");
            expected.push(format!("{entry}/{name}"));
        }
        let indices = format!("{pdir}/indices/");
        let mut actual: Vec<String> = list_tree(&cache_dir)
            .into_iter()
            .filter(|p| {
                p != "last-used-timestamps.db"
                    && p != ".locks/last-used.lock"
                    && p != ".locks/replace.lock"
            })
            .map(|p| match p.strip_prefix(&indices) {
                Some(rest) => {
                    let tail = rest.split_once('/').map(|(_, t)| t);
                    match tail {
                        Some(t) => format!("{indices}<h>/{t}"),
                        None => format!("{indices}<h>"),
                    }
                }
                None => p,
            })
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);

        assert_eq!(
            read_bytes(&cache_dir.join(&entry).join("logs.cache")).await,
            LOGS_CONTENT.to_vec()
        );
        let meta: CachedTaskExecution = rmp_serde::decode::from_slice(
            &std::fs::read(cache_dir.join(&entry).join("cache.meta.bin"))
                .expect("meta"),
        )
        .expect("metadata is msgpack CachedTaskExecution");
        assert_eq!(meta.logs_path, Some(PathBuf::from("logs.cache")));
        assert!(meta.files.iter().all(|f| f.cached_path.is_relative()));
        assert_eq!(meta.digest, cached.digest);
    }

    /// An entry laid out by hand exactly as the previous version wrote it
    /// (relative cached paths, metadata last, no extra files) must be read,
    /// left alone by an equal-outcome re-cache and upgradable from failure.
    #[tokio::test]
    async fn test_reads_entry_written_by_previous_version() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        // Learn the digest and output file names, then wipe the entry.
        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .expect("failed to cache");
        let learned = cache.get(task.get()).await.expect("get").expect("some");
        let entry_dir = dir
            .join(".omni/cache")
            .join(path_safe("project1"))
            .join("output")
            .join(bs58::encode(&learned.digest).into_string());
        tokio::fs::remove_dir_all(&entry_dir).await.expect("rm");
        assert!(cache.get(task.get()).await.expect("get").is_none());

        // Hand-build the legacy entry.
        std::fs::create_dir_all(&entry_dir).expect("mkdir");
        std::fs::write(entry_dir.join("logs.cache"), &LOGS_CONTENT)
            .expect("logs");
        let mut files = vec![];
        for f in &learned.files {
            // `get` resolves `original_path` to an absolute path, but the
            // previous version stored it unresolved and anchored to a root.
            let rooted = omni_types::OmniPath::new_rooted(
                f.original_path
                    .path()
                    .expect("resolved")
                    .strip_prefix(dir.join("project1"))
                    .expect("inside the project"),
                omni_types::Root::Project,
            );
            let name = format!("{}.cache", hashtext(&rooted.to_string()));
            std::fs::write(entry_dir.join(&name), JS_CONTENT).expect("file");
            files.push(CachedFileOutput {
                cached_path: PathBuf::from(name),
                original_path: rooted,
            });
        }
        let legacy = CachedTaskExecution {
            execution_time: OffsetDateTime::UNIX_EPOCH,
            files,
            logs_path: Some(PathBuf::from("logs.cache")),
            exit_code: 1,
            ..learned.clone()
        };
        let legacy_bytes = rmp_serde::encode::to_vec(&legacy).expect("enc");
        std::fs::write(entry_dir.join("cache.meta.bin"), &legacy_bytes)
            .expect("meta");

        let got = cache
            .get(task.get())
            .await
            .expect("get")
            .expect("legacy entry must be readable");
        assert_eq!(got.exit_code, 1);
        assert_eq!(got.digest, learned.digest);
        assert_eq!(
            read_bytes(got.logs_path.as_ref().expect("logs")).await,
            LOGS_CONTENT.to_vec()
        );
        for f in &got.files {
            assert_eq!(read_bytes(&f.cached_path).await, JS_CONTENT.as_bytes());
        }

        // A repeated failure leaves the legacy entry byte-for-byte alone.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                1,
            ))
            .await
            .expect("recache failure");
        assert_eq!(
            std::fs::read(entry_dir.join("cache.meta.bin")).expect("meta"),
            legacy_bytes
        );

        // A success upgrades it in place, with no staging leftovers.
        cache
            .cache(&new_cache_info_with_exit_code(
                Some(&LOGS_CONTENT),
                *task.get(),
                0,
            ))
            .await
            .expect("upgrade");
        let upgraded = cache.get(task.get()).await.expect("get").expect("some");
        assert_eq!(upgraded.exit_code, 0);
        assert!(
            list_tree(entry_dir.parent().expect("parent"))
                .iter()
                .all(|p| !p.starts_with(STAGING_PREFIX))
        );
    }

    #[tokio::test]
    async fn test_logs_path_should_not_exist_if_no_logs() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(None, *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *project.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task1.get()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task2.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task1.get()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task2.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task1.get()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task2.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task1.get()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task2.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task1.get()))
            .await
            .expect("failed to cache");

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task2.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
                    .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
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

    // A leading `!` in a stats project glob is a literal character here, not a
    // negation marker. `!other` matches only the literal name "!other", so
    // "project1" is not selected. This guards against negation leaking into
    // get_stats during the include_set migration. When get_stats deliberately
    // gains negation, this assertion no longer holds and the test should be
    // removed or inverted.
    #[tokio::test]
    async fn test_get_stats_leading_bang_is_literal() {
        let temp = fixture(&["project1"]).await;
        let dir = temp.path();
        let cache = cache_store(dir);
        let task = task("task", "project1", dir);

        cache
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *task.get()))
            .await
            .expect("failed to cache");

        let stats = cache
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &["!other"],
                ..Default::default()
            })
            .await
            .expect("failed to get stats");

        assert!(
            stats.projects.iter().all(|p| p.project_name != "project1"),
            "a leading `!` must not negate: `!other` selects no project"
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
            .cache(&new_cache_info(Some(&LOGS_CONTENT), *seed_task.get()))
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
    // End to end security scenarios against the real server router.
    include!("e2e_security.rs");
}

#[cfg(test)]
mod diagnostic_tests {
    use omni_core::ProjectGraphError;
    use omni_diagnostic::find;
    use omni_task_context::TaskContextProviderError;

    use super::*;

    #[test]
    fn task_context_diagnostics_reach_the_local_store_error() {
        let coded = eyre::Report::new(ProjectGraphError::not_found("web"));
        let err = LocalTaskExecutionCacheStoreError::from(
            LocalTaskExecutionCacheStoreErrorInner::TaskContextProvider(
                TaskContextProviderError::from(coded),
            ),
        );
        let r = eyre::Report::new(err);
        assert_eq!(find(&r).unwrap().code(), "project.not_found");
    }

    #[test]
    fn plumbing_failures_stay_uncoded() {
        let io = eyre::Report::new(LocalTaskExecutionCacheStoreError::from(
            LocalTaskExecutionCacheStoreErrorInner::Io(std::io::Error::other(
                "x",
            )),
        ));
        assert!(find(&io).is_none());

        let missing =
            eyre::Report::new(LocalTaskExecutionCacheStoreError::from(
                LocalTaskExecutionCacheStoreErrorInner::MissingContext,
            ));
        assert!(find(&missing).is_none());
    }
}
