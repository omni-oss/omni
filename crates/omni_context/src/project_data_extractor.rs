use std::path::{Path, PathBuf};

use config_utils::{IntoInner, ListConfig};
use derive_new::new;
use env::{
    CommandExpansionConfig, ExpansionError, expand_into_with_command_config,
};
use env_loader::EnvLoaderError;
use maps::{Map, UnorderedMap};
use merge::Merge as _;
use omni_config_types::MergeSingleOrMany;
use omni_configurations::{
    MetaConfiguration, ProjectConfiguration, RedactPolicy,
    WorkspaceConfiguration,
};
use omni_core::{
    ExtensionGraph, ExtensionGraphError, ExtensionGraphNode as _, Project, Task,
};
use omni_glob_config::MergeGlobConfig;
use omni_task_context::CacheInfo;
use omni_task_output_logs::OutputLogsConfiguration;
use omni_types::OmniPathError;
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tracing::Level;

use crate::{
    EnvLoader, EnvVarsMap, GetVarsArgs, build,
    env_loader::EnvCacheSys,
    project_config_entry::is_runnable_project,
    utils::{EnvVarsOsMap, vars_os},
};

#[derive(new)]
pub struct ProjectDataExtractor<'a, TSys: EnvCacheSys> {
    sys: &'a TSys,
    root_dir: &'a Path,
    env_loader: &'a mut EnvLoader<TSys>,
    inherit_env_vars: bool,
    workspace_configuration: &'a WorkspaceConfiguration,
}

impl<'a, TSys: EnvCacheSys> ProjectDataExtractor<'a, TSys> {
    fn get_ws_vars_vars(
        &self,
    ) -> Result<(EnvVarsMap, EnvVarsOsMap), ProjectDataExtractorError> {
        if self.workspace_configuration.env.vars.is_empty() {
            return Ok((EnvVarsMap::default(), EnvVarsOsMap::default()));
        }

        let base_vars = if self.inherit_env_vars {
            self.sys.env_vars().collect::<Map<_, _>>()
        } else {
            Map::default()
        };

        let base_vars_os = vars_os(&base_vars);

        let cfg =
            CommandExpansionConfig::new_enabled(self.root_dir, &base_vars_os);

        let mut ws_vars = self.workspace_configuration.env.vars.clone();
        expand_into_with_command_config(&mut ws_vars, &base_vars, &cfg)?;

        let ws_vars_os = vars_os(&ws_vars);

        Ok((ws_vars, ws_vars_os))
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub fn extract_all(
        &mut self,
        project_configs: &[ProjectConfiguration],
        project_paths: &[PathBuf],
        extension_graph: &ExtensionGraph<ProjectConfiguration>,
    ) -> Result<ProjectDataExtractions, ProjectDataExtractorError> {
        let mut projects = vec![];
        let mut project_meta_configs = maps::unordered_map![];
        let mut task_meta_configs = maps::unordered_map![];
        let mut task_env_var_overrides = maps::unordered_map![];
        let mut cache_infos = maps::unordered_map![];
        let mut output_logs_configs = maps::unordered_map![];
        let mut redact_policies = maps::unordered_map![];

        let project_paths = project_paths
            .iter()
            .map(|p| p as &Path)
            .collect::<UnorderedSet<_>>();

        let root_dir = self.root_dir.to_string_lossy().to_string();

        let (ws_vars, ws_vars_os) = self.get_ws_vars_vars()?;

        self.workspace_configuration
            .redact
            .check(true)
            .map_err(|message| {
                ProjectDataExtractorErrorInner::InvalidRedact {
                    location: "the workspace configuration".to_string(),
                    message,
                }
            })?;

        let filtered = project_configs
            .iter()
            .filter(|config| is_runnable_project(config, &project_paths));

        let ws_extras = maps::map![
            "WORKSPACE_DIR".to_string() => root_dir.clone(),
            "OMNI_VERSION".to_string() => build::PKG_VERSION.to_string(),
            "PLATFORM_HOST".to_string() => target_lexicon::HOST.to_string(),
            "PLATFORM_OS".to_string() => target_lexicon::HOST.operating_system.to_string(),
            "PLATFORM_ARCH".to_string() => target_lexicon::HOST.architecture.to_string(),
            "PLATFORM_ENV".to_string() => target_lexicon::HOST.environment.to_string(),
            "PLATFORM_VENDOR".to_string() => target_lexicon::HOST.vendor.to_string(),
            "PLATFORM_BINARY_FORMAT".to_string() => target_lexicon::HOST.binary_format.to_string(),
        ];

        for project_config in filtered {
            tracing::debug!(
                project_configuration = ?project_config,
                "processing_project_config",
            );

            let dir =
                project_config.dir.path().expect("path should be resolved");

            let mut project_extras = maps::map![
                "PROJECT_NAME".to_string() => project_config.name.to_string(),
                "PROJECT_DIR".to_string() => dir.to_string_lossy().to_string(),
            ];

            project_extras.extend(ws_extras.clone());

            let overrides = &project_config.env.vars;
            if !overrides.as_map().is_empty() {
                project_extras.extend(overrides.to_map_to_inner());
                if !ws_vars_os.is_empty() {
                    let cfg = CommandExpansionConfig::new_enabled(
                        project_config.dir.path()?,
                        &ws_vars_os,
                    );
                    expand_into_with_command_config(
                        &mut project_extras,
                        &self.workspace_configuration.env.vars,
                        &cfg,
                    )?;

                    project_extras.extend(ws_vars.clone());
                }
            }

            // load the env vars for the project
            _ = self.env_loader.get(&GetVarsArgs {
                start_dir: Some(dir),
                project_env_var_overrides: Some(&project_extras),
                inherit_env_vars: self.inherit_env_vars,
            })?;

            let project_cache = &project_config.cache;
            let project_output_logs = &project_config.output_logs;
            let project_meta = &project_config.meta;

            project_config.redact.check(false).map_err(|message| {
                ProjectDataExtractorErrorInner::InvalidRedact {
                    location: format!("project '{}'", project_config.name),
                    message,
                }
            })?;

            project_meta_configs
                .insert(project_config.name.clone(), project_meta.clone());

            for (name, task) in project_config.tasks.iter() {
                let full_task_name =
                    format!("{}#{}", project_config.name, name);
                if let Some(env) = task.env()
                    && let Some(vars) = env.vars.as_ref()
                {
                    task_env_var_overrides
                        .insert(full_task_name.clone(), vars.to_map_to_inner());
                }

                let task_cache = task.cache();
                let task_output_logs = task.output_logs();

                let output_logs = match (project_output_logs, task_output_logs)
                {
                    (Some(project), Some(task)) => {
                        let mut resolved = *project;
                        resolved.merge(*task);
                        Some(resolved)
                    }
                    (Some(project), None) => Some(*project),
                    (None, Some(task)) => Some(*task),
                    (None, None) => None,
                };

                if let Some(output_logs) = output_logs {
                    output_logs_configs
                        .insert(full_task_name.clone(), output_logs);
                }

                let cache = if let Some(cache_key) = task_cache {
                    let mut pc = project_cache.clone();
                    pc.merge(cache_key.clone());
                    pc
                } else {
                    project_cache.clone()
                };

                let use_defaults = cache.key.defaults.unwrap_or(true);

                let key_input_files = if use_defaults {
                    let mut files = cache.key.files.clone();
                    let mut additional_files = extension_graph
                        .get_transitive_extendee_ids(project_config.id())?;

                    additional_files.push(project_config.id().clone());

                    // The project's own config-file ids seed the include side
                    // only; defaults are always includes. The exclude side
                    // is omitted so the project's excludes survive.
                    files.merge(MergeGlobConfig::IncludeAndExclude {
                        include: Some(MergeSingleOrMany::List(
                            ListConfig::prepend(additional_files),
                        )),
                        exclude: None,
                    });
                    files.normalize()
                } else {
                    cache.key.files.clone().normalize()
                };

                cache_infos.insert(
                    full_task_name.clone(),
                    CacheInfo {
                        cache_enabled: cache
                            .enabled
                            .map(|i| i.into_inner())
                            .unwrap_or(true),
                        key_defaults: use_defaults,
                        key_env_keys: cache.key.env.to_vec_to_inner(),
                        key_input_files,
                        cache_output_files: cache
                            .output
                            .files
                            .clone()
                            .normalize(),
                        cache_logs: cache.output.logs,
                        args: task
                            .args()
                            .map(|args| {
                                args.iter()
                                    .map(|(k, v)| {
                                        (k.clone(), v.clone().into_json())
                                    })
                                    .collect::<Map<_, _>>()
                            })
                            .unwrap_or_default(),
                    },
                );

                let task_meta = task.meta();

                let task_meta = if let Some(meta) = task_meta {
                    let meta = meta.clone();
                    let mut project_meta = project_meta.clone();
                    project_meta.merge(meta);
                    project_meta
                } else {
                    project_meta.clone()
                };

                if let Some(task_redact) = task.redact() {
                    task_redact.check(false).map_err(|message| {
                        ProjectDataExtractorErrorInner::InvalidRedact {
                            location: format!(
                                "task '{}' in project '{}'",
                                name, project_config.name
                            ),
                            message,
                        }
                    })?;
                }

                let mut levels = vec![
                    &self.workspace_configuration.redact,
                    &project_config.redact,
                ];
                if let Some(task_redact) = task.redact() {
                    levels.push(task_redact);
                }
                redact_policies.insert(
                    full_task_name.clone(),
                    RedactPolicy::resolve(&levels),
                );

                task_meta_configs.insert(full_task_name, task_meta);
            }

            projects.push(Project::new(
                project_config.name.clone(),
                dir.to_path_buf(),
                project_config.dependencies.to_vec_inner(),
                project_config
                    .tasks
                    .iter()
                    .map(|(task_name, v)| {
                        let mapped: Task = v.clone().get_task(task_name);

                        (task_name.clone(), mapped)
                    })
                    .collect(),
            ));
        }

        Ok(ProjectDataExtractions::new(
            projects,
            cache_infos,
            task_env_var_overrides,
            project_meta_configs,
            task_meta_configs,
            output_logs_configs,
            redact_policies,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, new, Serialize, Deserialize)]
pub struct ProjectDataExtractions {
    pub projects: Vec<Project>,
    pub cache_infos: UnorderedMap<String, CacheInfo>,
    pub task_env_var_overrides: UnorderedMap<String, Map<String, String>>,
    pub project_meta_configs: UnorderedMap<String, MetaConfiguration>,
    pub task_meta_configs: UnorderedMap<String, MetaConfiguration>,
    pub output_logs_configs: UnorderedMap<String, OutputLogsConfiguration>,
    /// Resolved redaction policy per `project#task`. Display only: never
    /// hashed and never part of cache keys.
    #[serde(default)]
    pub redact_policies: UnorderedMap<String, RedactPolicy>,
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct ProjectDataExtractorError(pub(crate) ProjectDataExtractorErrorInner);

impl ProjectDataExtractorError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectDataExtractorErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectDataExtractorErrorInner>> From<T>
    for ProjectDataExtractorError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ProjectDataExtractorErrorKind))]
pub(crate) enum ProjectDataExtractorErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExtensionGraph(#[from] ExtensionGraphError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    EnvLoader(#[from] EnvLoaderError),

    #[error(transparent)]
    #[diagnostic(skip)]
    CommandExpansion(#[from] ExpansionError),

    #[error(transparent)]
    #[diagnostic(skip)]
    OmniPath(#[from] OmniPathError),

    /// A `redact` configuration is invalid.
    ///
    /// The `redact` block of a workspace, project or task failed validation.
    #[error("invalid `redact` configuration in {location}: {message}")]
    #[diagnostic(
        code = "workspace.invalid_redact",
        hint = "fix the `redact` configuration in {location}"
    )]
    InvalidRedact {
        #[diagnostic(detail)]
        location: String,
        #[diagnostic(detail = "reason")]
        message: String,
    },
}
