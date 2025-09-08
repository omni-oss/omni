use std::path::{Path, PathBuf};

use config_utils::ListConfig;
use derive_new::new;
use env_loader::EnvLoaderError;
use maps::{Map, UnorderedMap};
use merge::Merge as _;
use omni_configurations::{
    MetaConfiguration, ProjectConfiguration, TaskOutputConfiguration,
};
use omni_core::{
    ExtensionGraph, ExtensionGraphError, ExtensionGraphNode as _, Project, Task,
};
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    CacheInfo, EnvLoader, GetVarsArgs, build, env_loader::EnvCacheSys,
};

#[derive(new)]
pub struct ProjectDataExtractor<'a, TSys: EnvCacheSys> {
    root_dir: &'a Path,
    env_loader: &'a mut EnvLoader<TSys>,
    inherit_env_vars: bool,
}

impl<'a, TSys: EnvCacheSys> ProjectDataExtractor<'a, TSys> {
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

        let project_paths = project_paths
            .iter()
            .map(|p| p as &Path)
            .collect::<UnorderedSet<_>>();

        let root_dir = self.root_dir.to_string_lossy().to_string();

        for project_config in project_configs.iter().filter(|config| {
            !config.base
                && project_paths.contains(
                    config.file.path().expect("path should be resolved"),
                )
        }) {
            trace::trace!(
                project_configuration = ?project_config,
                "processing project config: {}",
                project_config.name
            );

            let dir =
                project_config.dir.path().expect("path should be resolved");
            let mut extras = maps::map![
                "WORKSPACE_DIR".to_string() => root_dir.clone(),
                "PROJECT_NAME".to_string() => project_config.name.to_string(),
                "PROJECT_DIR".to_string() => dir.to_string_lossy().to_string(),
                "OMNI_VERSION".to_string() => build::PKG_VERSION.to_string(),
            ];

            let overrides = &project_config.env.vars;
            if !overrides.as_map().is_empty() {
                extras.extend(overrides.to_map_to_inner());
            }

            let env_files = project_config
                .env
                .files
                .as_vec()
                .iter()
                .map::<&Path, _>(|s| s)
                .collect::<Vec<_>>();

            // load the env vars for the project
            _ = self.env_loader.get(&GetVarsArgs {
                start_dir: Some(dir),
                project_env_var_overrides: Some(&extras),
                env_files: if env_files.is_empty() {
                    Some(&env_files[..])
                } else {
                    None
                },
                inherit_env_vars: self.inherit_env_vars,
            })?;

            let project_cache = &project_config.cache;
            let meta_config = &project_config.meta;

            project_meta_configs
                .insert(project_config.name.clone(), meta_config.clone());

            for (name, task) in project_config.tasks.iter() {
                if let Some(env) = task.env()
                    && let Some(vars) = env.vars.as_ref()
                {
                    let key = format!("{}#{}", project_config.name, name);

                    task_env_var_overrides.insert(key, vars.to_map_to_inner());
                }

                let task_cache = task.cache();
                let task_output = task.output().cloned().unwrap_or_else(|| {
                    TaskOutputConfiguration {
                        files: ListConfig::append(vec![]),
                        logs: true,
                    }
                });

                let cache = if let Some(cache_key) = task_cache {
                    let mut pc = project_cache.clone();
                    pc.merge(cache_key.clone());
                    pc
                } else {
                    project_cache.clone()
                };

                let use_defaults = cache.key.defaults.unwrap_or(true);

                let key_files = if use_defaults {
                    let mut files = cache.key.files.clone();
                    let mut additional_files = extension_graph
                        .get_transitive_extendee_ids(project_config.id())?;

                    additional_files.push(project_config.id().clone());

                    files.merge(ListConfig::prepend(additional_files));
                    files.to_vec()
                } else {
                    cache.key.files.to_vec()
                };

                cache_infos.insert(
                    format!("{}#{}", project_config.name, name),
                    CacheInfo {
                        cache_execution: cache.enabled,
                        key_defaults: use_defaults,
                        key_env_keys: cache.key.env.to_vec_to_inner(),
                        key_input_files: key_files,
                        cache_output_files: task_output.files.to_vec(),
                        cache_logs: task_output.logs,
                    },
                );

                let meta = task.meta();

                let meta = if let Some(meta) = meta {
                    let mut meta = meta.clone();
                    meta.merge(meta_config.clone());
                    meta
                } else {
                    meta_config.clone()
                };

                task_meta_configs
                    .insert(format!("{}#{}", project_config.name, name), meta);
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
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ProjectDataExtractorError {
    #[source]
    inner: ProjectDataExtractorErrorInner,
    kind: ProjectDataExtractorErrorKind,
}

impl ProjectDataExtractorError {
    pub fn kind(&self) -> ProjectDataExtractorErrorKind {
        self.kind
    }
}

impl<T: Into<ProjectDataExtractorErrorInner>> From<T>
    for ProjectDataExtractorError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ProjectDataExtractorErrorKind))]
enum ProjectDataExtractorErrorInner {
    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    ExtensionGraph(#[from] ExtensionGraphError),

    #[error(transparent)]
    EnvLoader(#[from] EnvLoaderError),
}
