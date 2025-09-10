use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use derive_new::new;
use env::expand_into;
use env_loader::EnvLoaderError;
use omni_configurations::{MetaConfiguration, WorkspaceConfiguration};
use omni_core::{Project, ProjectGraph, ProjectGraphError, TaskExecutionNode};
use omni_hasher::impls::DefaultHash;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    CacheInfo, Context, ContextSys, EnvLoader, GetVarsArgs,
    project_data_extractor::ProjectDataExtractions,
    project_query::ProjectQuery,
    utils::EnvVarsMap,
    workspace_hasher::{WorkspaceHasher, WorkspaceHasherError},
};

#[derive(Clone, Debug, new)]
pub struct LoadedContext<TSys: ContextSys = RealSys> {
    env_loader: EnvLoader<TSys>,
    unloaded_context: Context<TSys>,
    extracted: ProjectDataExtractions,
}

impl<TSys: ContextSys> LoadedContext<TSys> {
    pub fn sys(&self) -> &TSys {
        self.unloaded_context.sys()
    }

    pub fn env_files(&self) -> &[String] {
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

    pub fn workspace_configuration(&self) -> &WorkspaceConfiguration {
        self.unloaded_context.workspace_configuration()
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

    pub fn get_project_graph(&self) -> Result<ProjectGraph, ProjectGraphError> {
        let projects = self.projects().to_vec();

        ProjectGraph::from_projects(projects)
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
    ) -> Option<Arc<EnvVarsMap>> {
        let cached = self.env_loader.get_cached(task.project_dir());
        let overrides = self
            .extracted
            .task_env_var_overrides
            .get(task.full_task_name());

        match (cached, overrides) {
            (None, None) => None,
            (None, Some(overrides)) => Some(Arc::new(overrides.clone())),
            (Some(cached), None) => Some(cached),
            (Some(cached), Some(overrides)) => {
                let mut cached = (*cached).clone();
                let mut overrides = overrides.clone();

                expand_into(&mut overrides, &cached);
                cached.extend(overrides);
                Some(Arc::new(cached))
            }
        }
    }

    fn cache_dir(&self) -> PathBuf {
        self.unloaded_context.root_dir().join(".omni/cache")
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
}
