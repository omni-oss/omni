use std::{path::PathBuf, time::Duration};

use bytes::Bytes;
use derive_new::new;
use omni_cache::{
    CachedTaskExecution, CachedTaskExecutionHash, NewCacheInfo,
    TaskExecutionCacheStore, TaskExecutionInfoExt as _,
};
use omni_collector::{CollectConfig, Collector, CollectorSys, ProjectTaskInfo};
use omni_process::TaskChildProcessResult;
use omni_task_context::TaskContext;
use omni_types::TaskMap;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Force;

#[derive(Debug, bon::Builder, new)]
pub struct CacheManager<TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: CollectorSys,
{
    store: TCacheStore,

    #[builder(default = false)]
    dry_run: bool,

    #[builder(default = Force::None)]
    force: Force,

    #[builder(default = false)]
    no_cache: bool,

    sys: TSys,

    root_dir: PathBuf,

    cache_dir: PathBuf,

    /// A store that never touches the remote cache and keeps its entries in
    /// the `local` partition. Tasks in `local_only` use it instead of `store`.
    #[new(default)]
    local_store: Option<TCacheStore>,

    /// `(project, task)` pairs that must stay off the remote cache for this
    /// run: every task that opted out with `cache.remote: false` (or all of
    /// them under `--no-remote-cache`) and everything they depend on.
    #[builder(default)]
    #[new(default)]
    local_only: TaskMap<()>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, new)]
pub enum TaskResultContext<'a> {
    Completed {
        task_context: &'a TaskContext<'a>,
        result: TaskChildProcessResult,
        tries: u16,
    },
    Error {
        task_context: &'a TaskContext<'a>,
        /// The failure as shown to users, already sanitized for the task's
        /// operation. It outlives the task's redactor, so it is redacted
        /// before the operation ends (RFC 0026).
        message: String,
        tries: u16,
    },
}

impl<'a> TaskResultContext<'a> {
    pub fn task_context(&self) -> &TaskContext<'a> {
        match self {
            TaskResultContext::Completed { task_context, .. }
            | TaskResultContext::Error { task_context, .. } => task_context,
        }
    }

    pub fn exit_code(&self) -> u32 {
        match self {
            TaskResultContext::Completed { result, .. } => result.exit_code,
            TaskResultContext::Error { .. } => 1,
        }
    }

    pub fn tries(&self) -> u16 {
        match self {
            TaskResultContext::Completed { tries: t, .. } => *t,
            TaskResultContext::Error { tries: t, .. } => *t,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        match self {
            TaskResultContext::Completed { result, .. } => result.elapsed,
            TaskResultContext::Error { .. } => Duration::ZERO,
        }
    }

    pub fn logs(&self) -> Option<&Bytes> {
        match self {
            TaskResultContext::Completed { result, .. } => result.logs.as_ref(),
            TaskResultContext::Error { .. } => None,
        }
    }
}

impl<TCacheStore, TSys> CacheManager<TCacheStore, TSys>
where
    TCacheStore: TaskExecutionCacheStore,
    TSys: CollectorSys,
{
    pub async fn get_cached_results<'a>(
        &self,
        inputs: &[impl AsRef<TaskContext<'a>>],
    ) -> Result<TaskMap<CachedTaskExecution>, CacheManagerError> {
        if self.force.is_all() {
            return Ok(TaskMap::new());
        }

        let inputs = inputs
            .iter()
            .filter(|i| is_cache_eligible(i.as_ref()))
            .filter_map(|i| i.as_ref().execution_info())
            .collect::<Vec<_>>();

        if inputs.is_empty() {
            return Ok(TaskMap::new());
        }

        let (local, remote): (Vec<_>, Vec<_>) =
            inputs.into_iter().partition(|i| {
                self.local_only.contains(i.project_name, i.task_name)
            });
        // The two stores use separate partitions and locks, so they are
        // queried at the same time. Local-only tasks never fall back to the
        // store that talks to the remote: without a local store they simply
        // miss. Both passes run to completion so an error in one never
        // cancels the other halfway through a restore.
        let from_remote = async {
            if remote.is_empty() {
                return Ok(Vec::new());
            }
            self.store.get_many(&remote).await.map_err(|e| {
                CacheManagerError::from(
                    CacheManagerErrorInner::GetCacheFailed { source: e.into() },
                )
            })
        };
        let from_local = async {
            match &self.local_store {
                Some(local_store) if !local.is_empty() => {
                    local_store.get_many(&local).await.map_err(|e| {
                        CacheManagerError::from(
                            CacheManagerErrorInner::GetCacheFailed {
                                source: e.into(),
                            },
                        )
                    })
                }
                _ => Ok(Vec::new()),
            }
        };
        let (from_remote, from_local) =
            futures::future::join(from_remote, from_local).await;
        let mut cached_items = from_remote?;
        cached_items.extend(from_local?);

        let mut results = TaskMap::new();
        for c in cached_items.into_iter().flatten() {
            if c.exit_code != 0 && self.force.is_failed() {
                continue;
            }
            results.insert(&c.project_name.clone(), &c.task_name.clone(), c);
        }

        Ok(results)
    }

    pub async fn cache_results<'a>(
        &'a self,
        cache_contexts: &'a [TaskResultContext<'a>],
    ) -> Result<TaskMap<CachedTaskExecutionHash<'a>>, CacheManagerError> {
        if self.no_cache || cache_contexts.is_empty() {
            return Ok(TaskMap::new());
        }

        let to_cache = cache_contexts
            .iter()
            .filter_map(|r| {
                if is_cache_eligible(r.task_context())
                    && let Some(exec_info) = r.task_context().execution_info()
                {
                    Some(NewCacheInfo {
                        execution_duration: r.elapsed(),
                        exit_code: r.exit_code(),
                        task: exec_info,
                        logs: r.logs(),
                        tries: r.tries(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if self.dry_run {
            let collect_task_infos = to_cache
                .iter()
                .map(|info| ProjectTaskInfo {
                    input_files: info.task.input_files,
                    input_files_exclude: info.task.input_files_exclude,
                    output_files: info.task.output_files,
                    output_files_exclude: info.task.output_files_exclude,
                    project_dir: info.task.project_dir,
                    project_name: info.task.project_name,
                    task_exec: info.task.task_exec,
                    task_retry_exec: info.task.task_retry_exec,
                    task_name: info.task.task_name,
                    dependency_digests: info.task.dependency_digests,
                    env_vars: info.task.env_vars,
                    input_env_keys: info.task.input_env_keys,
                    args: info.task.args,
                })
                .collect::<Vec<_>>();

            let collector = Collector::new(
                &self.root_dir,
                &self.cache_dir,
                self.sys.clone(),
            );

            let results = collector
                .collect(
                    &collect_task_infos,
                    &CollectConfig {
                        digests: true,
                        ..Default::default()
                    },
                )
                .await?;

            let mut hashes = TaskMap::new();
            for c in results {
                hashes.insert(
                    c.task.project_name,
                    c.task.task_name,
                    CachedTaskExecutionHash {
                        digest: c
                            .digest
                            .expect("should have digest at this point"),
                        project_name: c.task.project_name,
                        task_name: c.task.task_name,
                    },
                );
            }
            Ok(hashes)
        } else {
            let (local, remote): (Vec<_>, Vec<_>) =
                to_cache.into_iter().partition(|i| {
                    self.local_only
                        .contains(i.task.project_name, i.task.task_name)
                });
            // Concurrent for the same reason as in `get_cached_results`.
            let to_remote = async {
                if remote.is_empty() {
                    return Ok(Vec::new());
                }
                self.store.cache_many(&remote).await.map_err(|e| {
                    CacheManagerError::from(
                        CacheManagerErrorInner::CacheFailed {
                            source: e.into(),
                        },
                    )
                })
            };
            let to_local = async {
                match &self.local_store {
                    Some(local_store) if !local.is_empty() => {
                        local_store.cache_many(&local).await.map_err(|e| {
                            CacheManagerError::from(
                                CacheManagerErrorInner::CacheFailed {
                                    source: e.into(),
                                },
                            )
                        })
                    }
                    _ => Ok(Vec::new()),
                }
            };
            let (to_remote, to_local) =
                futures::future::join(to_remote, to_local).await;
            let mut cached_items = to_remote?;
            cached_items.extend(to_local?);

            let mut hashes = TaskMap::new();
            for c in cached_items {
                hashes.insert(c.project_name, c.task_name, c);
            }
            Ok(hashes)
        }
    }
}

/// The tasks that must stay off the remote cache: every task for which
/// `opted_out` is true, or all of them when `all` is set (`--no-remote-cache`),
/// and everything they transitively depend on. A task a remote entry could feed
/// into an opted-out task would defeat the opt-out, so the dependencies of an
/// opted-out task are local-only too, even when other tasks also use them.
/// Only dependencies that are in the plan count: a dependency that is not run
/// in this invocation (pattern filters, `enabled: false`, or
/// `ignore_dependencies`, where only the opted-out tasks count) leaves its
/// workspace outputs as they are, which may be content an earlier run restored
/// from the remote cache.
pub(crate) fn local_only_closure<'a>(
    nodes: impl IntoIterator<Item = &'a omni_core::TaskExecutionNode>,
    opted_out: impl Fn(&omni_core::TaskExecutionNode) -> bool,
    all: bool,
    ignore_dependencies: bool,
) -> TaskMap<()> {
    let mut by_key: TaskMap<&omni_core::TaskExecutionNode> = TaskMap::new();
    for node in nodes {
        by_key.insert(node.project_name(), node.task_name(), node);
    }
    let mut closure = TaskMap::new();
    let mut stack: Vec<&omni_core::TaskExecutionNode> = by_key
        .values()
        .copied()
        .filter(|n| all || opted_out(n))
        .collect();
    while let Some(node) = stack.pop() {
        if closure
            .insert(node.project_name(), node.task_name(), ())
            .is_some()
        {
            continue;
        }
        if ignore_dependencies {
            continue;
        }
        for dep in node.dependencies() {
            if let Some(dep) = by_key.get(dep.project(), dep.task()) {
                stack.push(dep);
            }
        }
    }
    closure
}

/// The local-only set for a run. Empty unless remote caching is configured:
/// without it every task is local already and keeps the plain cache layout.
/// `opted_out(project, task)` says whether a task set `cache.remote: false`.
pub(crate) fn local_only_set<'a>(
    nodes: impl IntoIterator<Item = &'a omni_core::TaskExecutionNode>,
    remote_enabled: bool,
    opted_out: impl Fn(&str, &str) -> bool,
    no_remote_cache: bool,
    ignore_dependencies: bool,
) -> TaskMap<()> {
    if !remote_enabled {
        return TaskMap::new();
    }
    local_only_closure(
        nodes,
        |node| opted_out(node.project_name(), node.task_name()),
        no_remote_cache,
        ignore_dependencies,
    )
}

/// Whether a task may read, replay or write execution-cache entries.
pub(crate) fn is_cache_eligible(task: &TaskContext<'_>) -> bool {
    task.cache_info.as_ref().is_some_and(|ci| ci.cache_enabled)
        && !task.node.persistent()
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct CacheManagerError(pub(crate) CacheManagerErrorInner);

impl CacheManagerError {
    #[allow(unused)]
    pub fn kind(&self) -> CacheManagerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<CacheManagerErrorInner>> From<T> for CacheManagerError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(CacheManagerErrorKind), vis(pub))]
pub(crate) enum CacheManagerErrorInner {
    #[error("failed to get cached results")]
    #[diagnostic(delegate)]
    GetCacheFailed {
        #[source]
        source: eyre::Report,
    },

    #[error("failed to cache results")]
    #[diagnostic(delegate)]
    CacheFailed {
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Collector(#[from] omni_collector::error::Error),
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, path::PathBuf, sync::Arc};

    use omni_cache::TaskExecutionInfoExt as _;
    use omni_command_config::CommandConfig;
    use omni_config_types::TeraExprBoolean;
    use omni_configurations::CacheEnabledSource;
    use omni_core::TaskExecutionNode;
    use omni_task_context::{CacheInfo, TaskContext};
    use omni_types::{TaskKey, TaskMap};

    use super::{is_cache_eligible, local_only_closure, local_only_set};

    fn node(persistent: bool) -> TaskExecutionNode {
        TaskExecutionNode::new(
            TaskKey::new("p", "t"),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            persistent,
            Default::default(),
        )
    }

    fn pairs(list: &[(&str, &str)]) -> TaskMap<()> {
        let mut set = TaskMap::new();
        for (p, t) in list {
            set.insert(p, t, ());
        }
        set
    }

    fn info(enabled: bool) -> CacheInfo {
        CacheInfo {
            cache_enabled: enabled,
            cache_enabled_source: Default::default(),
            cache_remote: true,
            key_defaults: true,
            key_env_keys: vec![],
            key_input_files: Default::default(),
            cache_output_files: Default::default(),
            cache_logs: false,
            args: Default::default(),
            env: Default::default(),
        }
    }

    /// The digest the provider would compute for this node and info.
    fn digest_of(
        node: &TaskExecutionNode,
        info: Option<&CacheInfo>,
    ) -> omni_hasher::impls::DefaultHash {
        omni_task_context::effective_task::build(node, info, None).digest()
    }

    fn eligible(node: &TaskExecutionNode, info: Option<CacheInfo>) -> bool {
        let ctx = TaskContext {
            node,
            dependency_hashes: vec![],
            env_vars: Arc::new(Default::default()),
            args: Default::default(),
            effective_digest: digest_of(node, info.as_ref()),
            cache_info: info.map(Cow::Owned),
            output_logs: None,
            template_context: Default::default(),
        };
        is_cache_eligible(&ctx)
    }

    fn named(project: &str, task: &str, deps: &[&str]) -> TaskExecutionNode {
        TaskExecutionNode::new(
            TaskKey::new(project, task),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from(""),
            deps.iter()
                .map(|d| {
                    let (p, t) = d.split_once('#').expect("project#task");
                    TaskKey::new(p, t)
                })
                .collect(),
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        )
    }

    fn closure(
        nodes: &[TaskExecutionNode],
        opted_out: &[&str],
        all: bool,
        ignore_dependencies: bool,
    ) -> Vec<String> {
        let mut out: Vec<String> = local_only_closure(
            nodes,
            |n| opted_out.contains(&n.full_task_name()),
            all,
            ignore_dependencies,
        )
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();
        out.sort();
        out
    }

    #[test]
    fn local_only_opt_out_of_a_pulled_in_sibling_reaches_its_prerequisite() {
        let plan = crate::test_plan::app_plan();
        let nodes = plan.iter().flatten().cloned().collect::<Vec<_>>();
        assert_eq!(
            closure(&nodes, &["tool#build-native"], false, false),
            ["tool#build-native", "tool#setup"]
        );
    }

    #[test]
    fn the_closure_is_the_opted_out_tasks_and_everything_they_depend_on() {
        // build -> test -> release; lint is unrelated; docs depends on build.
        let nodes = [
            named("p", "build", &[]),
            named("p", "test", &["p#build"]),
            named("p", "release", &["p#test"]),
            named("p", "lint", &[]),
            named("p", "docs", &["p#build"]),
        ];
        assert_eq!(
            closure(&nodes, &["p#release"], false, false),
            ["p#build", "p#release", "p#test"],
            "dependencies of an opted-out task are local-only, dependents and unrelated tasks are not"
        );
        assert_eq!(closure(&nodes, &[], false, false), Vec::<String>::new());
        assert_eq!(closure(&nodes, &["p#lint"], false, false), ["p#lint"]);
        // docs only depends on build: opting docs out pulls build in, not test.
        assert_eq!(
            closure(&nodes, &["p#docs"], false, false),
            ["p#build", "p#docs"]
        );
    }

    #[test]
    fn a_diamond_and_a_shared_dependency_are_visited_once() {
        let nodes = [
            named("p", "base", &[]),
            named("p", "left", &["p#base"]),
            named("p", "right", &["p#base"]),
            named("p", "top", &["p#left", "p#right"]),
        ];
        assert_eq!(
            closure(&nodes, &["p#top"], false, false),
            ["p#base", "p#left", "p#right", "p#top"]
        );
        // Two opted-out tasks that share a dependency.
        assert_eq!(
            closure(&nodes, &["p#left", "p#right"], false, false),
            ["p#base", "p#left", "p#right"]
        );
    }

    #[test]
    fn no_remote_cache_and_ignored_dependencies() {
        let nodes = [named("p", "a", &[]), named("p", "b", &["p#a"])];
        assert_eq!(closure(&nodes, &[], true, false), ["p#a", "p#b"]);
        assert_eq!(
            closure(&nodes, &["p#b"], false, true),
            ["p#b"],
            "with ignored dependencies only the task itself"
        );
        // A dependency that is not in the plan is skipped, not an error.
        let nodes = [named("p", "b", &["p#missing"])];
        assert_eq!(closure(&nodes, &["p#b"], false, false), ["p#b"]);
    }

    #[test]
    fn an_opted_out_pair_does_not_pull_in_the_formerly_colliding_pair() {
        let ab = TaskExecutionNode::new(
            TaskKey::new("a#b", "c"),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        );
        let a = TaskExecutionNode::new(
            TaskKey::new("a", "b#c"),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from(""),
            vec![],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        );
        let app = TaskExecutionNode::new(
            TaskKey::new("app", "x"),
            Some(CommandConfig::Shell("echo".to_string())),
            None,
            PathBuf::from(""),
            vec![TaskKey::new("a#b", "c")],
            TeraExprBoolean::Boolean(true),
            false,
            false,
            Default::default(),
        );
        let nodes = [ab, a, app];
        let set = local_only_set(
            &nodes,
            true,
            |project, task| project == "app" && task == "x",
            false,
            false,
        );
        assert!(set.contains("app", "x"));
        assert!(
            set.contains("a#b", "c"),
            "the real dependency is local-only"
        );
        assert!(
            !set.contains("a", "b#c"),
            "the other pair must still be allowed to use the remote"
        );
    }

    #[test]
    fn the_set_is_built_from_the_opt_out_flag_and_only_with_remote_caching() {
        let nodes = [
            named("p", "build", &[]),
            named("p", "release", &["p#build"]),
            named("p", "lint", &[]),
        ];
        let opted_out = |_: &str, task: &str| task == "release";
        let set = |remote, all, ignore| {
            let mut v: Vec<String> =
                local_only_set(&nodes, remote, opted_out, all, ignore)
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect();
            v.sort();
            v
        };
        assert_eq!(set(true, false, false), ["p#build", "p#release"]);
        assert_eq!(set(true, false, true), ["p#release"]);
        assert_eq!(set(true, true, false), ["p#build", "p#lint", "p#release"]);
        // Without remote caching nothing changes layout.
        assert!(set(false, false, false).is_empty());
        assert!(set(false, true, false).is_empty());
        // A task that did not opt out is not local-only.
        let none = local_only_set(&nodes, true, |_, _| false, false, false);
        assert!(none.is_empty());
    }

    #[derive(Default, Clone)]
    struct SpyStore {
        gets: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        puts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        /// When set, every lookup is a hit with this exit code.
        hit_exit_code: Option<u32>,
        /// When set, every call waits here until the other store arrives, so a
        /// sequential caller would never get past it.
        rendezvous: Option<std::sync::Arc<tokio::sync::Barrier>>,
    }

    #[async_trait::async_trait]
    impl omni_cache::TaskExecutionCacheStore for SpyStore {
        type Error = std::io::Error;

        async fn cache_many<'a>(
            &'a self,
            infos: &[omni_cache::NewCacheInfo<'a>],
        ) -> Result<Vec<omni_cache::CachedTaskExecutionHash<'a>>, Self::Error>
        {
            if let Some(barrier) = &self.rendezvous {
                barrier.wait().await;
            }
            let mut out = Vec::new();
            for info in infos {
                self.puts.lock().unwrap().push(format!(
                    "{}#{}",
                    info.task.project_name, info.task.task_name
                ));
                out.push(omni_cache::CachedTaskExecutionHash::new(
                    info.task.project_name,
                    info.task.task_name,
                    [0u8; 32],
                ));
            }
            Ok(out)
        }

        async fn get_many(
            &self,
            infos: &[omni_cache::TaskExecutionInfo],
        ) -> Result<Vec<Option<omni_cache::CachedTaskExecution>>, Self::Error>
        {
            if let Some(barrier) = &self.rendezvous {
                barrier.wait().await;
            }
            for info in infos {
                self.gets
                    .lock()
                    .unwrap()
                    .push(format!("{}#{}", info.project_name, info.task_name));
            }
            Ok(infos
                .iter()
                .map(|info| {
                    self.hit_exit_code.map(|exit_code| {
                        omni_cache::CachedTaskExecution::new(
                            info.project_name,
                            info.task_name,
                            None::<String>,
                            None::<String>,
                            [0u8; 32],
                            Vec::<omni_hasher::impls::DefaultHash>::new(),
                            None::<PathBuf>,
                            Vec::<omni_cache::CachedFileOutput>::new(),
                            exit_code,
                            std::time::Duration::ZERO,
                            time::OffsetDateTime::UNIX_EPOCH,
                            1u16,
                        )
                    })
                })
                .collect())
        }

        async fn prune_caches<TContext: omni_cache::Context>(
            &self,
            _: &omni_cache::PruneCacheArgs<'_, TContext>,
        ) -> Result<Vec<omni_cache::PrunedCacheEntry>, Self::Error> {
            unimplemented!()
        }

        async fn force_prune_caches(
            &self,
            _: &[omni_cache::PrunedCacheEntry],
        ) -> Result<(), Self::Error> {
            unimplemented!()
        }

        async fn get_stats<TContext: omni_cache::Context>(
            &self,
            _: &omni_cache::CacheStatsArgs<'_, TContext>,
        ) -> Result<omni_cache::CacheStats, Self::Error> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn local_only_tasks_never_reach_the_store_that_talks_to_the_remote() {
        let remote = SpyStore::default();
        let local = SpyStore::default();
        let manager = super::CacheManager::<
            SpyStore,
            system_traits::impls::RealSys,
        >::builder()
        .store(remote.clone())
        .maybe_local_store(Some(local.clone()))
        .local_only(pairs(&[("p", "release"), ("p", "build")]))
        .sys(system_traits::impls::RealSys)
        .root_dir(PathBuf::new())
        .cache_dir(PathBuf::new())
        .build();

        let nodes = [
            named("p", "build", &[]),
            named("p", "release", &["p#build"]),
            named("p", "lint", &[]),
        ];
        let contexts: Vec<Cow<'_, TaskContext<'_>>> = nodes
            .iter()
            .map(|node| {
                Cow::Owned(TaskContext {
                    node,
                    dependency_hashes: vec![],
                    env_vars: Arc::new(Default::default()),
                    args: Default::default(),
                    effective_digest: digest_of(node, Some(&info(true))),
                    cache_info: Some(Cow::Owned(info(true))),
                    output_logs: None,
                    template_context: Default::default(),
                })
            })
            .collect();

        manager.get_cached_results(&contexts).await.unwrap();
        let mut got_remote = remote.gets.lock().unwrap().clone();
        let mut got_local = local.gets.lock().unwrap().clone();
        got_remote.sort();
        got_local.sort();
        assert_eq!(got_remote, ["p#lint"]);
        assert_eq!(got_local, ["p#build", "p#release"]);

        let results: Vec<_> = nodes
            .iter()
            .zip(&contexts)
            .map(|(node, ctx)| super::TaskResultContext::Completed {
                task_context: ctx.as_ref(),
                result: omni_process::TaskChildProcessResult::new(
                    node.clone(),
                    0u32,
                    std::time::Duration::ZERO,
                    None::<bytes::Bytes>,
                ),
                tries: 1,
            })
            .collect();
        manager.cache_results(&results).await.unwrap();
        assert_eq!(*remote.puts.lock().unwrap(), ["p#lint"]);
        let mut put_local = local.puts.lock().unwrap().clone();
        put_local.sort();
        assert_eq!(put_local, ["p#build", "p#release"]);
    }

    #[tokio::test]
    async fn hits_from_both_stores_are_merged_and_force_failed_filters_both() {
        let nodes = [named("p", "release", &[]), named("p", "lint", &[])];
        let contexts: Vec<Cow<'_, TaskContext<'_>>> = nodes
            .iter()
            .map(|node| {
                Cow::Owned(TaskContext {
                    node,
                    dependency_hashes: vec![],
                    env_vars: Arc::new(Default::default()),
                    args: Default::default(),
                    effective_digest: digest_of(node, Some(&info(true))),
                    cache_info: Some(Cow::Owned(info(true))),
                    output_logs: None,
                    template_context: Default::default(),
                })
            })
            .collect();
        for (exit_code, force, expected) in [
            (0, crate::Force::None, vec!["p#lint", "p#release"]),
            (1, crate::Force::None, vec!["p#lint", "p#release"]),
            (1, crate::Force::Failed, vec![]),
            (0, crate::Force::Failed, vec!["p#lint", "p#release"]),
        ] {
            let hit = |store: SpyStore| SpyStore {
                hit_exit_code: Some(exit_code),
                ..store
            };
            let manager = super::CacheManager::<
                SpyStore,
                system_traits::impls::RealSys,
            >::builder()
            .store(hit(SpyStore::default()))
            .maybe_local_store(Some(hit(SpyStore::default())))
            .local_only(pairs(&[("p", "release")]))
            .force(force)
            .sys(system_traits::impls::RealSys)
            .root_dir(PathBuf::new())
            .cache_dir(PathBuf::new())
            .build();
            let mut got: Vec<String> = manager
                .get_cached_results(&contexts)
                .await
                .unwrap()
                .iter()
                .map(|(k, _)| k.to_string())
                .collect();
            got.sort();
            assert_eq!(got, expected, "{exit_code} {force:?}");
        }
    }

    #[tokio::test]
    async fn the_remote_and_local_stores_are_queried_and_written_concurrently()
    {
        // Each store waits for the other, so a sequential caller hangs.
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let spy = || SpyStore {
            rendezvous: Some(barrier.clone()),
            ..SpyStore::default()
        };
        let manager = super::CacheManager::<
            SpyStore,
            system_traits::impls::RealSys,
        >::builder()
        .store(spy())
        .maybe_local_store(Some(spy()))
        .local_only(pairs(&[("p", "release")]))
        .sys(system_traits::impls::RealSys)
        .root_dir(PathBuf::new())
        .cache_dir(PathBuf::new())
        .build();
        let nodes = [named("p", "release", &[]), named("p", "lint", &[])];
        let contexts: Vec<Cow<'_, TaskContext<'_>>> = nodes
            .iter()
            .map(|node| {
                Cow::Owned(TaskContext {
                    node,
                    dependency_hashes: vec![],
                    env_vars: Arc::new(Default::default()),
                    args: Default::default(),
                    effective_digest: digest_of(node, Some(&info(true))),
                    cache_info: Some(Cow::Owned(info(true))),
                    output_logs: None,
                    template_context: Default::default(),
                })
            })
            .collect();
        let limit = std::time::Duration::from_secs(5);

        tokio::time::timeout(limit, manager.get_cached_results(&contexts))
            .await
            .expect("the two stores must be queried at the same time")
            .unwrap();

        let results: Vec<_> = nodes
            .iter()
            .zip(&contexts)
            .map(|(node, ctx)| super::TaskResultContext::Completed {
                task_context: ctx.as_ref(),
                result: omni_process::TaskChildProcessResult::new(
                    node.clone(),
                    0u32,
                    std::time::Duration::ZERO,
                    None::<bytes::Bytes>,
                ),
                tries: 1,
            })
            .collect();
        let cached =
            tokio::time::timeout(limit, manager.cache_results(&results))
                .await
                .expect("the two stores must be written at the same time")
                .unwrap();
        assert_eq!(cached.len(), 2, "both stores' results are returned");
    }

    #[tokio::test]
    async fn without_a_local_store_a_local_only_task_misses_instead_of_using_the_remote()
     {
        let remote = SpyStore::default();
        let manager = super::CacheManager::<
            SpyStore,
            system_traits::impls::RealSys,
        >::builder()
        .store(remote.clone())
        .local_only(pairs(&[("p", "release")]))
        .sys(system_traits::impls::RealSys)
        .root_dir(PathBuf::new())
        .cache_dir(PathBuf::new())
        .build();
        let nodes = [named("p", "release", &[])];
        let contexts: Vec<Cow<'_, TaskContext<'_>>> = nodes
            .iter()
            .map(|node| {
                Cow::Owned(TaskContext {
                    node,
                    dependency_hashes: vec![],
                    env_vars: Arc::new(Default::default()),
                    args: Default::default(),
                    effective_digest: digest_of(node, Some(&info(true))),
                    cache_info: Some(Cow::Owned(info(true))),
                    output_logs: None,
                    template_context: Default::default(),
                })
            })
            .collect();
        manager.get_cached_results(&contexts).await.unwrap();
        assert!(remote.gets.lock().unwrap().is_empty());
    }

    #[test]
    fn enabled_non_persistent_is_eligible() {
        assert!(eligible(&node(false), Some(info(true))));
    }

    #[test]
    fn disabled_task_is_not_eligible() {
        assert!(!eligible(&node(false), Some(info(false))));
    }

    #[test]
    fn persistent_task_is_not_eligible() {
        assert!(!eligible(&node(true), Some(info(true))));
    }

    #[test]
    fn task_without_cache_info_is_not_eligible() {
        assert!(!eligible(&node(false), None));
    }

    fn context<'a>(
        node: &'a TaskExecutionNode,
        info: CacheInfo,
    ) -> TaskContext<'a> {
        TaskContext {
            node,
            dependency_hashes: vec![],
            env_vars: Arc::new(Default::default()),
            args: Cow::Owned(info.args.clone()),
            effective_digest: digest_of(node, Some(&info)),
            cache_info: Some(Cow::Owned(info)),
            output_logs: None,
            template_context: Default::default(),
        }
    }

    const SOURCES: [CacheEnabledSource; 5] = [
        CacheEnabledSource::HostFallback,
        CacheEnabledSource::ActivationDefaults,
        CacheEnabledSource::Plugin,
        CacheEnabledSource::ProjectExplicit,
        CacheEnabledSource::TaskExplicit,
    ];

    /// Provenance must never change a cache key: the same task decided by a
    /// different tier is the same task. `execution_info` is the whole of what the
    /// key is built from.
    #[test]
    fn the_origin_of_enablement_never_reaches_the_cache_key() {
        let node = node(false);
        let key = |source| {
            let info = CacheInfo {
                cache_enabled_source: source,
                ..info(true)
            };
            // Leak nothing: compare the derived execution info, not the context.
            let ctx = context(&node, info);
            let execution = ctx.execution_info().expect("has cache info");
            format!("{execution:?}")
        };
        let reference = key(SOURCES[0]);
        for source in SOURCES {
            assert_eq!(key(source), reference, "{source:?} changed the key");
        }
    }

    #[test]
    fn eligibility_depends_on_the_value_not_its_origin() {
        for source in SOURCES {
            let on = CacheInfo {
                cache_enabled_source: source,
                ..info(true)
            };
            let off = CacheInfo {
                cache_enabled_source: source,
                ..info(false)
            };
            assert!(eligible(&node(false), Some(on)), "{source:?}");
            assert!(!eligible(&node(false), Some(off)), "{source:?}");
        }
    }

    // RFC 0024 F06.3 proof: the final key must follow the effective task, not
    // only the raw text the collector hashes today. The helpers take the digest
    // from the two paths that exist for one task context: the dry run in this
    // module, and the hybrid store's own write path (the same `collect` that
    // lookup and stale pruning use).
    mod effective_key {
        use omni_cache::{
            NewCacheInfo, TaskExecutionCacheStore as _,
            impls::{HybridTaskExecutionCacheStore, RemoteConfig},
        };
        use omni_hasher::impls::DefaultHash;
        use omni_retry_config::RetryConfiguration;

        use super::*;
        use crate::cache_manager::{CacheManager, TaskResultContext};

        struct Fixture {
            temp: tempfile::TempDir,
        }

        impl Fixture {
            fn new() -> Self {
                let temp = tempfile::tempdir().unwrap();
                std::fs::create_dir_all(temp.path().join("p")).unwrap();
                Self { temp }
            }

            fn node(
                &self,
                retry: serde_json::Value,
                enabled: TeraExprBoolean,
            ) -> TaskExecutionNode {
                TaskExecutionNode::new(
                    TaskKey::new("p", "t"),
                    Some(CommandConfig::Shell("echo".to_string())),
                    None,
                    self.temp.path().join("p"),
                    vec![],
                    enabled,
                    false,
                    false,
                    serde_json::from_value::<RetryConfiguration>(retry)
                        .unwrap(),
                )
            }

            fn plain_node(&self) -> TaskExecutionNode {
                self.node(serde_json::json!({}), TeraExprBoolean::Boolean(true))
            }

            /// The digest the dry run reports for this context.
            async fn dry_run_digest(
                &self,
                ctx: &TaskContext<'_>,
            ) -> DefaultHash {
                let manager = CacheManager::<
                    SpyStore,
                    system_traits::impls::RealSys,
                >::builder()
                .store(SpyStore::default())
                .dry_run(true)
                .sys(system_traits::impls::RealSys)
                .root_dir(self.temp.path().to_path_buf())
                .cache_dir(self.temp.path().join("dry-cache"))
                .build();
                let result = TaskResultContext::Completed {
                    task_context: ctx,
                    result: omni_process::TaskChildProcessResult::new(
                        ctx.node.clone(),
                        0u32,
                        std::time::Duration::ZERO,
                        None::<bytes::Bytes>,
                    ),
                    tries: 1,
                };
                let results = [result];
                let hashes = manager.cache_results(&results).await.unwrap();
                hashes.get("p", "t").expect("a digest").digest
            }

            /// The digest the real store writes under for this context.
            async fn stored_digest(
                &self,
                ctx: &TaskContext<'_>,
            ) -> DefaultHash {
                let store = HybridTaskExecutionCacheStore::new(
                    self.temp.path().join("real-cache"),
                    self.temp.path(),
                    RemoteConfig::new_disabled(),
                );
                let task = ctx.execution_info().expect("cache info");
                let hashes = store
                    .cache_many(&[NewCacheInfo::new(
                        task,
                        None,
                        std::time::Duration::ZERO,
                        0,
                        1,
                    )])
                    .await
                    .unwrap();
                hashes[0].digest
            }
        }

        fn eligible_info() -> CacheInfo {
            CacheInfo {
                key_defaults: false,
                ..info(true)
            }
        }

        /// A changed effective setting that no ordinary input records: the
        /// task's retry policy.
        #[tokio::test]
        async fn a_retry_policy_change_changes_the_dry_run_key() {
            let f = Fixture::new();
            let (plain, retried) = (
                f.plain_node(),
                f.node(
                    serde_json::json!({"max_retries": 3}),
                    TeraExprBoolean::Boolean(true),
                ),
            );
            let a = f.dry_run_digest(&context(&plain, eligible_info())).await;
            let b = f.dry_run_digest(&context(&retried, eligible_info())).await;
            assert_ne!(
                a, b,
                "retry is effective semantics and must reach the key"
            );
        }

        #[tokio::test]
        async fn an_enabled_expression_change_changes_the_dry_run_key() {
            let f = Fixture::new();
            let (on, conditional) = (
                f.plain_node(),
                f.node(
                    serde_json::json!({}),
                    TeraExprBoolean::Expr("{{ env.CI }}".into()),
                ),
            );
            let a = f.dry_run_digest(&context(&on, eligible_info())).await;
            let b = f
                .dry_run_digest(&context(&conditional, eligible_info()))
                .await;
            assert_ne!(
                a, b,
                "enabled is effective semantics and must reach the key"
            );
        }

        /// The same change through the store's own path, which lookup, write
        /// and stale pruning share.
        #[tokio::test]
        async fn a_retry_policy_change_changes_the_stores_key() {
            let f = Fixture::new();
            let (plain, retried) = (
                f.plain_node(),
                f.node(
                    serde_json::json!({"max_retries": 3}),
                    TeraExprBoolean::Boolean(true),
                ),
            );
            let a = f.stored_digest(&context(&plain, eligible_info())).await;
            let b = f.stored_digest(&context(&retried, eligible_info())).await;
            assert_ne!(a, b, "the store's collect must see the effective task");
        }

        /// Parity: for the same task context the dry run and the store agree
        /// on the key. Holds on the base (neither sees the effective task) and
        /// must keep holding when both do; the retried variant is the case a
        /// half-plumbed change would break.
        #[tokio::test]
        async fn the_dry_run_and_the_store_agree_on_the_key() {
            let f = Fixture::new();
            let retried = f.node(
                serde_json::json!({"max_retries": 3}),
                TeraExprBoolean::Boolean(true),
            );
            for node in [f.plain_node(), retried] {
                let ctx = context(&node, eligible_info());
                assert_eq!(
                    f.dry_run_digest(&ctx).await,
                    f.stored_digest(&ctx).await
                );
            }
        }

        /// A context that returns the same configured task info and, per run,
        /// the invocation overrides (`omni run t --arg k=v`).
        struct ArgsCtx {
            info: CacheInfo,
            overrides: maps::UnorderedMap<String, serde_json::Value>,
        }

        impl omni_task_context::Context for ArgsCtx {
            type Error = std::io::Error;

            fn get_task_env_vars(
                &self,
                _: &TaskExecutionNode,
            ) -> Result<
                Option<std::sync::Arc<omni_task_context::EnvVars>>,
                Self::Error,
            > {
                Ok(Some(Default::default()))
            }

            fn get_task_meta_config(
                &self,
                _: &str,
                _: &str,
            ) -> Option<&omni_configurations::MetaConfiguration> {
                None
            }

            fn get_project_meta_config(
                &self,
                _: &str,
            ) -> Option<&omni_configurations::MetaConfiguration> {
                None
            }

            fn get_cache_info(&self, _: &str, _: &str) -> Option<&CacheInfo> {
                Some(&self.info)
            }

            fn get_output_logs(
                &self,
                _: &str,
                _: &str,
            ) -> Option<&omni_task_output_logs::OutputLogsConfiguration>
            {
                None
            }

            fn get_task_override_args(
                &self,
                _: &str,
                _: &str,
            ) -> Option<&maps::UnorderedMap<String, serde_json::Value>>
            {
                Some(&self.overrides)
            }
        }

        struct NoHashes;

        impl omni_task_context::TaskHashProvider for NoHashes {
            fn get_task_hash(
                &self,
                _: omni_types::TaskKeyRef<'_>,
            ) -> Option<DefaultHash> {
                None
            }
        }

        fn arg_override(
            value: Option<&str>,
        ) -> maps::UnorderedMap<String, serde_json::Value> {
            let mut map = maps::UnorderedMap::default();
            if let Some(value) = value {
                map.insert("k".to_string(), serde_json::json!(value));
            }
            map
        }

        /// FAILS on the base (Q1): `omni run t --arg k=v1` then `--arg k=v2`
        /// merges the override into the template context but not into the key,
        /// which sees only the configured `CacheInfo.args`. The second run
        /// replays the first result: an existing wrong hit.
        #[tokio::test]
        async fn a_changed_invocation_arg_changes_the_key() {
            let f = Fixture::new();
            let node = f.plain_node();
            let digest = |value: Option<&str>| {
                let provider =
                    omni_task_context::DefaultTaskContextProvider::new(
                        NoHashes,
                        ArgsCtx {
                            info: eligible_info(),
                            overrides: arg_override(value),
                        },
                    );
                let f = &f;
                let node = &node;
                async move {
                    use omni_task_context::TaskContextProvider as _;
                    let ctx = provider
                        .get_task_context(node, false)
                        .expect("a context");
                    (f.dry_run_digest(&ctx).await, f.stored_digest(&ctx).await)
                }
            };
            let none = digest(None).await;
            let v1 = digest(Some("v1")).await;
            let v2 = digest(Some("v2")).await;
            assert_ne!(v1.0, v2.0, "dry run: the merged args must be keyed");
            assert_ne!(v1.1, v2.1, "store: the merged args must be keyed");
            assert_ne!(none.1, v1.1, "an override is a different invocation");
        }

        /// Guard: with no override the key is today's key (hash project parity
        /// holds), and a repeated identical override is stable.
        #[tokio::test]
        async fn the_same_invocation_args_keep_the_key() {
            let f = Fixture::new();
            let node = f.plain_node();
            let digest = |value: Option<&str>| {
                let provider =
                    omni_task_context::DefaultTaskContextProvider::new(
                        NoHashes,
                        ArgsCtx {
                            info: eligible_info(),
                            overrides: arg_override(value),
                        },
                    );
                let f = &f;
                let node = &node;
                async move {
                    use omni_task_context::TaskContextProvider as _;
                    let ctx = provider
                        .get_task_context(node, false)
                        .expect("a context");
                    f.dry_run_digest(&ctx).await
                }
            };
            assert_eq!(digest(Some("v1")).await, digest(Some("v1")).await);
            assert_eq!(digest(None).await, digest(None).await);
            // No override: the key is the one built from the configured
            // `CacheInfo.args` alone, which `hash project` also uses.
            assert_eq!(
                digest(None).await,
                f.dry_run_digest(&context(&node, eligible_info())).await,
            );
        }

        /// Guards (pass on the base and must keep passing): presentation and
        /// provenance never reach the key, identical semantics keep it.
        #[tokio::test]
        async fn presentation_only_differences_keep_the_key() {
            let f = Fixture::new();
            let node = f.plain_node();
            let reference =
                f.dry_run_digest(&context(&node, eligible_info())).await;
            for source in SOURCES {
                let info = CacheInfo {
                    cache_enabled_source: source,
                    ..eligible_info()
                };
                assert_eq!(
                    f.dry_run_digest(&context(&node, info)).await,
                    reference,
                    "{source:?}"
                );
            }
            let remote_off = CacheInfo {
                cache_remote: false,
                ..eligible_info()
            };
            assert_eq!(
                f.dry_run_digest(&context(&node, remote_off)).await,
                reference
            );
        }
    }
}
