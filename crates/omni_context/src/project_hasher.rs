use std::{collections::HashSet, path::Path, sync::Arc};

use derive_new::new;
use globset::{Glob, GlobSetBuilder};
use maps::{Map, UnorderedMap, hash::HashMapExt};
use omni_collector::{CollectConfig, Collector, ProjectTaskInfo};
use omni_execution_plan::{Call, ExecutionPlanProvider};
use omni_hasher::{
    Hasher as _,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::Hash,
};
use omni_types::OmniPath;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::{ContextSys, LoadedContext, LoadedContextError};

#[derive(Debug, Clone, new)]
pub struct ProjectHasher<'a, TSys: ContextSys> {
    root_dir: &'a Path,
    cache_dir: &'a Path,
    sys: &'a TSys,
}

impl<'a, TSys: ContextSys> ProjectHasher<'a, TSys> {
    pub async fn hash<T: ExecutionPlanProvider>(
        &self,
        project_name: &str,
        task_names: &[&str],
        seed: Option<&str>,
        context: &LoadedContext<TSys>,
        execution_plan_provider: &T,
    ) -> Result<DefaultHash, ProjectHasherError> {
        let projects = context.projects();

        let seed =
            DefaultHasher::hash(seed.unwrap_or("DEFAULT_SEED").as_bytes());
        let call = Call::new_tasks(if task_names.is_empty() {
            vec!["**".to_string()]
        } else {
            task_names.iter().map(|s| s.to_string()).collect()
        });

        let plan = execution_plan_provider
            .get_execution_plan(
                &call,
                &[project_name],
                &[],
                None,
                None,
                false,
                false,
            )
            .map_err(|e| {
                ProjectHasherErrorInner::ExecutionPlanProvider(
                    eyre::Report::new(e),
                )
            })?;

        struct ProjectTaskInfoTmp<'a> {
            project_name: &'a str,
            project_dir: &'a Path,
            task_name: &'a str,
            task_command: &'a str,
            input_files: &'a [OmniPath],
            output_files: &'a [OmniPath],
            env_vars: Arc<Map<String, String>>,
            dependency_digests: Vec<DefaultHash>,
            input_env_keys: &'a [String],
        }

        #[derive(Debug, Clone)]
        struct DigestInfo {
            project_name: String,
            task_name: String,
            digest: DefaultHash,
        }

        let mut task_result_digests = UnorderedMap::new();

        for batch in plan.iter() {
            let mut task_infos_tmp = vec![];

            for task in batch.iter() {
                let project = projects
                    .iter()
                    .find(|p| p.name == task.project_name())
                    .ok_or_else(|| {
                        ProjectHasherErrorInner::NotFound(
                            task.project_name().to_owned(),
                        )
                    })?;

                let mut input_files = HashSet::new();

                let ci = context
                    .get_cache_info(task.project_name(), task.task_name())
                    .ok_or_else(|| {
                        ProjectHasherErrorInner::CacheInfoNotFound(
                            task.project_name().to_owned(),
                            task.task_name().to_owned(),
                        )
                    })?;
                input_files.extend(ci.key_input_files.clone());

                let env_vars = context
                    .get_task_env_vars(task)
                    .map_err(|e| {
                        ProjectHasherErrorInner::LoadedContext(Box::new(e))
                    })?
                    .ok_or_else(|| {
                        ProjectHasherErrorInner::TaskEnvVarsNotFound(
                            task.project_name().to_owned(),
                            task.task_name().to_owned(),
                        )
                    })?;

                task_infos_tmp.push(ProjectTaskInfoTmp {
                    project_dir: &project.dir,
                    project_name: &project.name,
                    task_name: task.task_name(),
                    task_command: project
                        .tasks
                        .get(task.task_name())
                        .map(|t| t.command.as_str())
                        .unwrap_or("default"),
                    output_files: &ci.cache_output_files,
                    input_files: &ci.key_input_files,
                    dependency_digests: task
                        .dependencies()
                        .iter()
                        .filter_map(|d| {
                            task_result_digests
                                .get(d)
                                .map(|x: &DigestInfo| x.digest)
                        })
                        .collect::<Vec<_>>(),
                    env_vars,
                    input_env_keys: &ci.key_env_keys,
                });
            }

            let task_infos = task_infos_tmp
                .iter()
                .map(|p| ProjectTaskInfo {
                    project_dir: p.project_dir,
                    project_name: p.project_name,
                    task_name: p.task_name,
                    task_command: p.task_command,
                    input_files: p.input_files,
                    output_files: p.output_files,
                    dependency_digests: &p.dependency_digests,
                    env_vars: &p.env_vars,
                    input_env_keys: p.input_env_keys,
                })
                .collect::<Vec<_>>();

            let collected =
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

            for c in collected {
                let key =
                    format!("{}#{}", c.task.project_name, c.task.task_name);
                task_result_digests.insert(
                    key,
                    DigestInfo {
                        digest: c.digest.expect("should have value"),
                        project_name: c.task.project_name.to_string(),
                        task_name: c.task.task_name.to_string(),
                    },
                );
            }
        }

        let mut hash = Hash::<DefaultHasher>::new(seed);

        let task_matcher = {
            let mut builder = GlobSetBuilder::new();

            for task_name in task_names {
                builder.add(Glob::new(task_name)?);
            }

            builder.build()?
        };

        let mut digests = task_result_digests
            .values()
            .filter(|d| {
                d.project_name == project_name
                    && if !task_matcher.is_empty() {
                        task_matcher.is_match(&d.task_name)
                    } else {
                        true
                    }
            })
            .collect::<Vec<_>>();

        digests.sort_by(|a, b| a.task_name.cmp(&b.task_name));

        for d in digests {
            hash.combine_in_place(d.digest);
        }

        Ok(hash.to_inner())
    }

    pub async fn hash_string<T: ExecutionPlanProvider>(
        &self,
        project_name: &str,
        task_names: &[&str],
        seed: Option<&str>,
        context: &LoadedContext<TSys>,
        execution_plan_provider: &T,
    ) -> Result<String, ProjectHasherError> {
        Ok(bs58::encode(
            self.hash(
                project_name,
                task_names,
                seed,
                context,
                execution_plan_provider,
            )
            .await?
            .as_ref(),
        )
        .into_string())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ProjectHasherError {
    #[source]
    inner: ProjectHasherErrorInner,
    kind: ProjectHasherErrorKind,
}

impl ProjectHasherError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectHasherErrorKind {
        self.kind
    }
}

impl<T: Into<ProjectHasherErrorInner>> From<T> for ProjectHasherError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(ProjectHasherErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
enum ProjectHasherErrorInner {
    #[error(transparent)]
    Collector(#[from] omni_collector::Error),

    #[error("Project `{0}` not found")]
    NotFound(String),

    #[error(transparent)]
    ExecutionPlanProvider(eyre::Report),

    #[error("Cache info not found for project `{0}` and task `{1}`")]
    CacheInfoNotFound(String, String),

    #[error("Task env vars not found for project `{0}` and task `{1}`")]
    TaskEnvVarsNotFound(String, String),

    #[error(transparent)]
    LoadedContext(#[from] Box<LoadedContextError>),

    #[error(transparent)]
    Glob(#[from] globset::Error),
}
