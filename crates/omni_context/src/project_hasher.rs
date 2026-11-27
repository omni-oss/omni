use std::{path::Path, sync::Arc};

use derive_new::new;
use maps::Map;
use omni_collector::{CollectConfig, Collector, ProjectTaskInfo};
use omni_command_config::CommandConfig;
use omni_execution_plan::{
    Call, ExecutionPlanProvider, ProjectSelector, TaskSelector,
};
use omni_hasher::{
    Hasher as _,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::Hash,
};
use omni_types::{OmniPath, TaskMap};
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
        tasks: &[TaskSelector],
        seed: Option<&str>,
        context: &LoadedContext<TSys>,
        execution_plan_provider: &T,
    ) -> Result<DefaultHash, ProjectHasherError> {
        let projects = context.projects();

        let seed =
            DefaultHasher::hash(seed.unwrap_or("DEFAULT_SEED").as_bytes());
        let call = if tasks.is_empty() {
            Call::all_tasks()
        } else {
            Call::Tasks(tasks.to_vec())
        };

        for selector in tasks {
            if let TaskSelector::Reference {
                project: Some(qualifier),
                task,
            } = selector
                && qualifier != project_name
            {
                return Err(ProjectHasherErrorInner::SelectorProjectMismatch {
                    selector: omni_types::task_label(qualifier, task),
                    project: project_name.to_owned(),
                }
                .into());
            }
        }

        let selection = execution_plan_provider
            .get_execution_selection(
                &call,
                &[ProjectSelector::exact(project_name)],
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

        let plan = selection.plan;
        let direct = selection.direct;

        struct ProjectTaskInfoTmp<'a> {
            project_name: &'a str,
            project_dir: &'a Path,
            task_name: &'a str,
            task_exec: Option<&'a CommandConfig>,
            task_retry_exec: Option<&'a CommandConfig>,
            input_files: &'a [OmniPath],
            input_files_exclude: &'a [OmniPath],
            output_files: &'a [OmniPath],
            output_files_exclude: &'a [OmniPath],
            env_vars: Arc<Map<String, String>>,
            dependency_digests: Vec<DefaultHash>,
            input_env_keys: &'a [String],
            args: &'a Map<String, serde_json::Value>,
            effective_digest: DefaultHash,
        }

        #[derive(Debug, Clone)]
        struct DigestInfo {
            task_name: String,
            digest: DefaultHash,
        }

        let mut task_result_digests: TaskMap<DigestInfo> = TaskMap::new();

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

                let ci = context
                    .get_cache_info(task.project_name(), task.task_name())
                    .ok_or_else(|| {
                        ProjectHasherErrorInner::CacheInfoNotFound(
                            task.project_name().to_owned(),
                            task.task_name().to_owned(),
                        )
                    })?;

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
                    // The plan node, as the executor reads it, so the command
                    // and the effective digest cannot diverge from a run.
                    task_exec: task.task_exec_config(),
                    task_retry_exec: task.task_retry_exec_config(),
                    output_files: &ci.cache_output_files.include,
                    output_files_exclude: &ci.cache_output_files.exclude,
                    input_files: &ci.key_input_files.include,
                    input_files_exclude: &ci.key_input_files.exclude,
                    dependency_digests: task
                        .dependencies()
                        .iter()
                        .filter_map(|d| {
                            task_result_digests
                                .get(d.project(), d.task())
                                .map(|x: &DigestInfo| x.digest)
                        })
                        .collect::<Vec<_>>(),
                    env_vars,
                    input_env_keys: &ci.key_env_keys,
                    args: &ci.args,
                    effective_digest: omni_task_context::effective_task::build(
                        task,
                        Some(ci),
                        context
                            .get_task_meta_config(
                                task.project_name(),
                                task.task_name(),
                            )
                            .or_else(|| {
                                context.get_project_meta_config(
                                    task.project_name(),
                                )
                            }),
                    )
                    .digest(),
                });
            }

            let task_infos = task_infos_tmp
                .iter()
                .map(|p| ProjectTaskInfo {
                    project_dir: p.project_dir,
                    project_name: p.project_name,
                    task_name: p.task_name,
                    task_exec: p.task_exec,
                    task_retry_exec: p.task_retry_exec,
                    input_files: p.input_files,
                    input_files_exclude: p.input_files_exclude,
                    output_files: p.output_files,
                    output_files_exclude: p.output_files_exclude,
                    dependency_digests: &p.dependency_digests,
                    env_vars: &p.env_vars,
                    input_env_keys: p.input_env_keys,
                    args: p.args,
                    effective_digest: p.effective_digest,
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
                task_result_digests.insert(
                    c.task.project_name,
                    c.task.task_name,
                    DigestInfo {
                        digest: c.digest.expect("should have value"),
                        task_name: c.task.task_name.to_string(),
                    },
                );
            }
        }

        let mut hash = Hash::<DefaultHasher>::new(seed);

        // Only the directly matched canonical tasks of this project are
        // aggregate inputs. Dependency digests already flow in through the
        // graph, and a dependency-only task is never folded in on its own.
        let mut digests = direct
            .iter()
            .filter(|key| key.project() == project_name)
            .filter_map(|key| {
                task_result_digests.get(key.project(), key.task())
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
        tasks: &[TaskSelector],
        seed: Option<&str>,
        context: &LoadedContext<TSys>,
        execution_plan_provider: &T,
    ) -> Result<String, ProjectHasherError> {
        Ok(bs58::encode(
            self.hash(
                project_name,
                tasks,
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

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct ProjectHasherError(pub(crate) ProjectHasherErrorInner);

impl ProjectHasherError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectHasherErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectHasherErrorInner>> From<T> for ProjectHasherError {
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
    name(ProjectHasherErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum ProjectHasherErrorInner {
    #[error(transparent)]
    #[diagnostic(skip)]
    Collector(#[from] omni_collector::Error),

    #[error("Project `{0}` not found")]
    /// A project that was asked to be hashed does not exist.
    #[diagnostic(
        code = "workspace.project_not_found",
        hint = "check the project name against `omni projects`"
    )]
    NotFound(#[diagnostic(detail = "project")] String),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExecutionPlanProvider(eyre::Report),

    #[error("Cache info not found for project `{0}` and task `{1}`")]
    #[diagnostic(skip)]
    CacheInfoNotFound(String, String),

    #[error("Task env vars not found for project `{0}` and task `{1}`")]
    #[diagnostic(skip)]
    TaskEnvVarsNotFound(String, String),

    #[error(transparent)]
    #[diagnostic(delegate)]
    LoadedContext(#[from] Box<LoadedContextError>),

    /// A project-qualified selector names a different project than the one
    /// being hashed.
    #[error(
        "selector `{selector}` names another project than `{project}`, which is being hashed"
    )]
    #[diagnostic(
        code = "workspace.selector_project_mismatch",
        hint = "drop the project qualifier or use `{project}` as the qualifier"
    )]
    SelectorProjectMismatch {
        #[diagnostic(detail)]
        selector: String,
        #[diagnostic(detail)]
        project: String,
    },
}
