use std::{collections::HashSet, path::Path};

use derive_new::new;
use maps::Map;
use omni_collector::{CollectConfig, Collector, CollectorSys, ProjectTaskInfo};
use omni_command_config::CommandConfig;
use omni_core::{ProjectGraph, ProjectGraphError, TaskExecutionGraphError};
use omni_hasher::{
    Hasher as _,
    impls::{DefaultHash, DefaultHasher, hash_parts},
    project_dir_hasher::Hash,
};
use omni_types::OmniPath;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::project_data_extractor::ProjectDataExtractions;

#[derive(Debug, Clone, new)]
pub struct WorkspaceHasher<'a, TSys: CollectorSys + Clone> {
    root_dir: &'a Path,
    cache_dir: &'a Path,
    sys: &'a TSys,
}

impl<'a, TSys: CollectorSys + Clone> WorkspaceHasher<'a, TSys> {
    pub async fn hash(
        &self,
        seed: Option<&str>,
        extracted_data: &ProjectDataExtractions,
    ) -> Result<DefaultHash, WorkspaceHasherError> {
        let projects = extracted_data.projects.as_slice();

        let seed = seed.unwrap_or("DEFAULT_SEED");
        let seed = DefaultHasher::hash(seed.as_bytes());

        let mut task_infos_tmp = Vec::with_capacity(projects.len());

        struct ProjectTaskInfoTmp<'a> {
            project_name: &'a str,
            project_dir: &'a Path,
            task_name: &'a str,
            task_exec: Option<&'a CommandConfig>,
            task_retry_exec: Option<&'a CommandConfig>,
            input_files: Vec<OmniPath>,
            env_vars: Map<String, String>,
            input_env_keys: Vec<String>,
            effective_digest: DefaultHash,
        }

        // The real task graph, so each task's effective digest is the one a
        // run and `hash project` compute. A graph that cannot be built (a
        // cycle, an invalid name) fails here with its own error instead of
        // hashing a partial set.
        let task_graph = ProjectGraph::from_projects(projects.to_vec())?
            .get_task_execution_graph()?;

        for project in projects {
            let mut input_files = HashSet::new();

            for (_, info) in
                extracted_data.cache_infos.project_tasks(&project.name)
            {
                input_files.extend(info.key_input_files.include.clone());
            }

            let input_files = input_files.into_iter().collect::<Vec<_>>();

            task_infos_tmp.push(ProjectTaskInfoTmp {
                project_dir: &project.dir,
                project_name: &project.name,
                task_name: "temp",
                task_exec: None,
                task_retry_exec: None,
                input_files,
                env_vars: maps::map![],
                input_env_keys: vec![],
                effective_digest: project_effective_digest(
                    project,
                    &task_graph,
                    extracted_data,
                )?,
            });
        }

        let args = maps::map![];

        let task_infos = task_infos_tmp
            .iter()
            .map(|p| ProjectTaskInfo {
                project_dir: p.project_dir,
                project_name: p.project_name,
                task_name: p.task_name,
                task_exec: p.task_exec,
                task_retry_exec: p.task_retry_exec,
                input_files: &p.input_files,
                input_files_exclude: &[],
                output_files: &[],
                output_files_exclude: &[],
                dependency_digests: &[],
                env_vars: &p.env_vars,
                input_env_keys: &p.input_env_keys,
                args: &args,
                effective_digest: p.effective_digest,
            })
            .collect::<Vec<_>>();

        let mut collected =
            Collector::new(self.root_dir, self.cache_dir, self.sys.clone())
                .collect(
                    &task_infos,
                    &CollectConfig {
                        input_files: true,
                        output_files: false,
                        digests: true,
                        cache_output_dirs: false,
                    },
                )
                .await?;

        // hash deterministically by sorting by hash
        collected.sort_by_key(|c| c.digest);

        let mut hash = Hash::<DefaultHasher>::new(seed);

        for collected in collected {
            hash.combine_in_place(
                collected.digest.as_ref().expect("should be some"),
            );
        }

        Ok(hash.to_inner())
    }

    pub async fn hash_string(
        &self,
        seed: Option<&str>,
        extracted_data: &ProjectDataExtractions,
    ) -> Result<String, WorkspaceHasherError> {
        Ok(
            bs58::encode(self.hash(seed, extracted_data).await?.as_ref())
                .into_string(),
        )
    }
}

/// What one project contributes to the workspace hash for its tasks: every task's
/// real effective digest, each as a domain-separated leaf naming the project
/// and the task (length-framed, so no rename or boundary shift collides), the
/// leaves sorted by task name. A project with no tasks folds an empty set under
/// its own domain tag, so it is not skipped and differs from every other
/// project.
fn project_effective_digest(
    project: &omni_core::Project,
    task_graph: &omni_core::TaskExecutionGraph,
    extracted_data: &ProjectDataExtractions,
) -> Result<DefaultHash, WorkspaceHasherError> {
    let mut names = project.tasks.keys().collect::<Vec<_>>();
    names.sort();

    let project_len = (project.name.len() as u64).to_le_bytes();
    let mut leaves = Vec::with_capacity(names.len());
    for task_name in names {
        let node = task_graph.get_task_by_name(&project.name, task_name)?;
        let digest = omni_task_context::effective_task::build(
            node,
            extracted_data.cache_infos.get(&project.name, task_name),
            extracted_data
                .task_meta_configs
                .get(&project.name, task_name)
                .or_else(|| {
                    extracted_data.project_meta_configs.get(&project.name)
                }),
        )
        .digest();
        let task_len = (task_name.len() as u64).to_le_bytes();
        leaves.push(hash_parts([
            b"omni.workspace-task\0".as_slice(),
            &project_len,
            project.name.as_bytes(),
            &task_len,
            task_name.as_bytes(),
            digest.as_slice(),
        ]));
    }

    let count = (leaves.len() as u64).to_le_bytes();
    let mut parts: Vec<&[u8]> = vec![
        b"omni.workspace-project\0".as_slice(),
        &project_len,
        project.name.as_bytes(),
        &count,
    ];
    parts.extend(leaves.iter().map(|l| l.as_slice()));
    Ok(hash_parts(parts))
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct WorkspaceHasherError(pub(crate) WorkspaceHasherErrorInner);

impl WorkspaceHasherError {
    #[allow(unused)]
    pub fn kind(&self) -> WorkspaceHasherErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<WorkspaceHasherErrorInner>> From<T> for WorkspaceHasherError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

#[derive(
    Debug,
    thiserror::Error,
    EnumDiscriminants,
    EnumIs,
    omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(
    name(WorkspaceHasherErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum WorkspaceHasherErrorInner {
    #[error(transparent)]
    #[diagnostic(skip)]
    Collector(#[from] omni_collector::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    TaskGraph(#[from] TaskExecutionGraphError),
}

#[cfg(test)]
mod tests {
    use omni_command_config::CommandConfig;
    use omni_config_types::TeraExprBoolean;
    use omni_core::{Project, Task, TaskDependency};
    use omni_task_context::CacheInfo;
    use omni_types::TaskMap;
    use system_traits::impls::RealSys;

    use super::*;

    fn task(exec: &str, deps: &[&str]) -> Task {
        Task::new(
            Some(CommandConfig::Shell(exec.into())),
            None,
            deps.iter()
                .map(|d| TaskDependency::Own {
                    task: d.to_string(),
                })
                .collect(),
            None,
            TeraExprBoolean::Boolean(true),
            false,
            false,
            vec![],
            Default::default(),
        )
    }

    fn info() -> CacheInfo {
        CacheInfo {
            cache_enabled: true,
            cache_enabled_source: Default::default(),
            cache_remote: true,
            key_defaults: false,
            key_env_keys: vec![],
            key_input_files: Default::default(),
            cache_output_files: Default::default(),
            cache_logs: false,
            args: Default::default(),
            env: Default::default(),
        }
    }

    fn extracted(
        dir: &Path,
        projects: &[(&str, &[(&str, &str, &[&str])])],
    ) -> ProjectDataExtractions {
        let mut cache_infos = TaskMap::new();
        let projects = projects
            .iter()
            .map(|(name, tasks)| {
                let mut project = Project::new(
                    *name,
                    dir.join(name),
                    vec![],
                    Default::default(),
                );
                for (task_name, exec, deps) in *tasks {
                    project
                        .tasks
                        .insert(task_name.to_string(), task(exec, deps));
                    cache_infos.insert(name, task_name, info());
                }
                project
            })
            .collect();
        ProjectDataExtractions::new(
            projects,
            cache_infos,
            TaskMap::new(),
            Default::default(),
            TaskMap::new(),
            TaskMap::new(),
            TaskMap::new(),
        )
    }

    async fn hash(
        dir: &Path,
        data: &ProjectDataExtractions,
    ) -> Result<DefaultHash, WorkspaceHasherError> {
        let cache = dir.join("cache");
        WorkspaceHasher::new(dir, &cache, &RealSys)
            .hash(None, data)
            .await
    }

    #[tokio::test]
    async fn an_effective_only_task_change_moves_the_workspace_hash() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let one = extracted(dir, &[("a", &[("t", "echo one", &[])])]);
        let two = extracted(dir, &[("a", &[("t", "echo two", &[])])]);
        let same = extracted(dir, &[("a", &[("t", "echo one", &[])])]);
        assert_eq!(
            hash(dir, &one).await.unwrap(),
            hash(dir, &same).await.unwrap()
        );
        assert_ne!(
            hash(dir, &one).await.unwrap(),
            hash(dir, &two).await.unwrap()
        );
    }

    #[tokio::test]
    async fn a_dependency_change_moves_the_workspace_hash() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let tasks: &[(&str, &str, &[&str])] =
            &[("t", "x", &[]), ("u", "x", &[])];
        let with_dep: &[(&str, &str, &[&str])] =
            &[("t", "x", &["u"]), ("u", "x", &[])];
        let a = extracted(dir, &[("a", tasks)]);
        let b = extracted(dir, &[("a", with_dep)]);
        assert_ne!(hash(dir, &a).await.unwrap(), hash(dir, &b).await.unwrap());
    }

    #[tokio::test]
    async fn a_project_with_no_tasks_still_contributes_and_a_rename_is_not_a_reorder()
     {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let none = extracted(dir, &[("a", &[])]);
        let other = extracted(dir, &[("b", &[])]);
        let both = extracted(dir, &[("a", &[]), ("b", &[])]);
        let (n, o, b) = (
            hash(dir, &none).await.unwrap(),
            hash(dir, &other).await.unwrap(),
            hash(dir, &both).await.unwrap(),
        );
        assert_ne!(n, o);
        assert_ne!(n, b);

        let ab = extracted(dir, &[("a", &[("t", "x", &[])]), ("b", &[])]);
        let ba = extracted(dir, &[("a", &[]), ("b", &[("t", "x", &[])])]);
        assert_ne!(
            hash(dir, &ab).await.unwrap(),
            hash(dir, &ba).await.unwrap()
        );
    }

    #[tokio::test]
    async fn a_cyclic_task_graph_fails_instead_of_hashing_a_partial_set() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let cyclic: &[(&str, &str, &[&str])] =
            &[("t", "x", &["u"]), ("u", "x", &["t"])];
        let data = extracted(dir, &[("a", cyclic)]);
        let error = hash(dir, &data).await.unwrap_err();
        assert!(error.kind().is_task_graph(), "{error}");
    }
}
