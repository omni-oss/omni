use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use derive_new::new;
use env::{
    CommandExpansionConfig, ExpansionError, expand_into_with_command_config,
};
use env_loader::EnvLoaderError;
use omni_configurations::{
    MetaConfiguration, RemoteCacheConfiguration, WorkspaceConfiguration,
};
use omni_core::{Project, ProjectGraph, ProjectGraphError, TaskExecutionNode};
use omni_execution_plan::DefaultExecutionPlanProvider;
use omni_hasher::impls::DefaultHash;
use omni_task_context::CacheInfo;
use omni_tracing_subscriber::TracingConfig;
use omni_types::OmniPath;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    Context, ContextSys, EnvLoader, GetVarsArgs,
    project_data_extractor::ProjectDataExtractions,
    project_hasher::{ProjectHasher, ProjectHasherError},
    project_query::ProjectQuery,
    utils::{EnvVarsMap, vars_os},
    workspace_hasher::{WorkspaceHasher, WorkspaceHasherError},
};

#[derive(Clone, Debug, new)]
pub struct LoadedContext<TSys: ContextSys = RealSys> {
    env_loader: EnvLoader<TSys>,
    unloaded_context: Context<TSys>,
    extracted: ProjectDataExtractions,
}

impl<TSys: ContextSys> LoadedContext<TSys> {
    pub fn tracing_config(&self) -> &TracingConfig {
        self.unloaded_context.tracing_config()
    }

    pub fn sys(&self) -> &TSys {
        self.unloaded_context.sys()
    }

    pub fn env_files(&self) -> &[PathBuf] {
        self.unloaded_context.env_files()
    }

    pub fn env_root_dir_marker(&self) -> &str {
        self.unloaded_context.env_root_dir_marker()
    }

    pub fn current_dir(&self) -> std::io::Result<PathBuf> {
        self.unloaded_context.current_dir()
    }

    pub fn root_dir(&self) -> &Path {
        self.unloaded_context.root_dir()
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.unloaded_context.cache_dir()
    }

    pub fn trace_dir(&self) -> PathBuf {
        self.unloaded_context.trace_dir()
    }

    pub fn workspace_configuration(&self) -> &WorkspaceConfiguration {
        self.unloaded_context.workspace_configuration()
    }

    pub fn remote_cache_configuration(
        &self,
    ) -> Option<&RemoteCacheConfiguration> {
        self.unloaded_context.remote_cache_configuration()
    }

    pub fn remote_cache_configuration_paths(&self) -> Vec<PathBuf> {
        self.unloaded_context.remote_cache_configuration_paths()
    }

    pub fn projects(&self) -> &[Project] {
        self.extracted.projects.as_slice()
    }

    pub fn query_projects(&self) -> ProjectQuery<'_> {
        ProjectQuery::new(self.projects())
    }

    pub fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo> {
        self.extracted
            .cache_infos
            .get(&format!("{project_name}#{task_name}"))
    }

    pub fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.extracted
            .task_meta_configs
            .get(&format!("{project_name}#{task_name}"))
    }

    pub fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.extracted.project_meta_configs.get(project_name)
    }

    pub async fn get_workspace_hash(
        &self,
    ) -> Result<DefaultHash, LoadedContextError> {
        let cache_dir = self.cache_dir();
        let hasher = self.get_workspace_hasher(&cache_dir)?;

        Ok(hasher.hash(Some(self.seed()), &self.extracted).await?)
    }

    pub async fn get_workspace_hash_string(
        &self,
    ) -> Result<String, LoadedContextError> {
        let cache_dir = self.cache_dir();
        let hasher = self.get_workspace_hasher(&cache_dir)?;

        Ok(hasher
            .hash_string(Some(self.seed()), &self.extracted)
            .await?)
    }

    fn get_execution_plan_provider<'a>(
        &'a self,
    ) -> DefaultExecutionPlanProvider<'a, ContextWrapper<'a, TSys>> {
        DefaultExecutionPlanProvider::new(ContextWrapper(self))
    }

    pub async fn get_project_hash(
        &self,
        project_name: &str,
        task_names: &[&str],
    ) -> Result<DefaultHash, LoadedContextError> {
        let cache_dir = self.cache_dir();
        let hasher = self.get_project_hasher(&cache_dir)?;
        let execution_plan_provider = self.get_execution_plan_provider();

        Ok(hasher
            .hash(
                project_name,
                task_names,
                None,
                self,
                &execution_plan_provider,
            )
            .await?)
    }

    pub async fn get_project_hash_string(
        &self,
        project_name: &str,
        task_names: &[&str],
    ) -> Result<String, LoadedContextError> {
        let cache_dir = self.cache_dir();
        let hasher = self.get_project_hasher(&cache_dir)?;
        let execution_plan_provider = self.get_execution_plan_provider();

        Ok(hasher
            .hash_string(
                project_name,
                task_names,
                None,
                self,
                &execution_plan_provider,
            )
            .await?)
    }

    pub fn get_project_graph(
        &self,
    ) -> Result<ProjectGraph, LoadedContextError> {
        let projects = self.projects().to_vec();

        Ok(ProjectGraph::from_projects(projects)?)
    }

    pub fn get_env_vars(
        &mut self,
        args: Option<&GetVarsArgs>,
    ) -> Result<Arc<EnvVarsMap>, EnvLoaderError> {
        let envs = self
            .env_loader
            .get(args.unwrap_or(&GetVarsArgs::default()))?;

        Ok(envs)
    }

    pub fn get_cached_env_vars(&self, path: &Path) -> Option<Arc<EnvVarsMap>> {
        self.env_loader.get_cached(path)
    }

    pub fn get_task_env_vars(
        &self,
        task: &TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVarsMap>>, LoadedContextError> {
        let cached = self.env_loader.get_cached(task.project_dir());
        let overrides = self
            .extracted
            .task_env_var_overrides
            .get(task.full_task_name());

        Ok(match (cached, overrides) {
            (None, None) => None,
            (None, Some(overrides)) => Some(Arc::new(overrides.clone())),
            (Some(cached), None) => Some(cached),
            (Some(cached), Some(overrides)) => {
                let mut cached = (*cached).clone();
                let mut overrides = overrides.clone();

                let vars = vars_os(&cached);
                let cfg = CommandExpansionConfig::new_enabled(
                    task.project_dir(),
                    &vars,
                );
                expand_into_with_command_config(&mut overrides, &cached, &cfg)?;

                cached.extend(overrides);
                Some(Arc::new(cached))
            }
        })
    }

    fn seed(&self) -> &str {
        self.unloaded_context
            .workspace_configuration()
            .name
            .as_deref()
            .unwrap_or("DEFAULT_SEED")
    }

    fn get_workspace_hasher<'a>(
        &'a self,
        cache_dir: &'a Path,
    ) -> Result<WorkspaceHasher<'a, TSys>, LoadedContextError> {
        let hasher = WorkspaceHasher::new(
            self.root_dir(),
            cache_dir,
            self.unloaded_context.sys(),
        );

        Ok(hasher)
    }

    fn get_project_hasher<'a>(
        &'a self,
        cache_dir: &'a Path,
    ) -> Result<ProjectHasher<'a, TSys>, LoadedContextError> {
        let hasher = ProjectHasher::new(
            self.root_dir(),
            cache_dir,
            self.unloaded_context.sys(),
        );

        Ok(hasher)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct LoadedContextError {
    #[source]
    inner: LoadedContextErrorInner,
    kind: LoadedContextErrorKind,
}

impl LoadedContextError {
    pub fn kind(&self) -> LoadedContextErrorKind {
        self.kind
    }
}

impl<T: Into<LoadedContextErrorInner>> From<T> for LoadedContextError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(LoadedContextErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
enum LoadedContextErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    EnvLoader(#[from] EnvLoaderError),

    #[error(transparent)]
    WorkspaceHasher(#[from] WorkspaceHasherError),

    #[error(transparent)]
    ProjectHasher(#[from] ProjectHasherError),

    #[error(transparent)]
    Expansion(#[from] ExpansionError),
}

// Private impls
pub struct ContextWrapper<'a, TSys: ContextSys>(&'a LoadedContext<TSys>);

impl<'a, TSys: ContextSys> omni_execution_plan::Context
    for ContextWrapper<'a, TSys>
{
    type Error = LoadedContextError;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.0.get_project_meta_config(project_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.0.get_task_meta_config(project_name, task_name)
    }

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error> {
        self.0.get_project_graph()
    }

    fn projects(&self) -> &[Project] {
        self.0.projects()
    }

    fn root_dir(&self) -> &Path {
        self.0.root_dir()
    }

    fn get_cache_input_files(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> &[OmniPath] {
        self.0
            .get_cache_info(project_name, task_name)
            .map(|c| &c.key_input_files[..])
            .unwrap_or(&[])
    }
}
