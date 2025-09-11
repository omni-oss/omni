use std::path::{Path, PathBuf};

pub(crate) use crate::env_loader::EnvLoader;
use env_loader::EnvLoaderError;
use omni_cache::impls::LocalTaskExecutionCacheStore;
use owo_colors::OwoColorize as _;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use trace::Level;

use crate::{
    ContextSys, LoadedContext,
    constants::{self, CACHE_DIR},
    extracted_data_validator::{
        ExtractedDataValidationErrors, ExtractedDataValidator,
    },
    project_config_loader::{ProjectConfigLoader, ProjectConfigLoaderError},
    project_data_extractor::{ProjectDataExtractor, ProjectDataExtractorError},
    project_discovery::{
        DiscoveredPath, ProjectDiscovery, ProjectDiscoveryError,
    },
};
use dir_walker::DirWalker;
use omni_core::{ExtensionGraph, ExtensionGraphError};
use system_traits::impls::RealSys as RealSysSync;

use omni_configurations::WorkspaceConfiguration;

#[derive(Clone, PartialEq, Debug)]
pub struct Context<TSys: ContextSys = RealSysSync> {
    env_root_dir_marker: String,
    env_files: Vec<String>,
    inherit_env_vars: bool,
    workspace: WorkspaceConfiguration,
    root_dir: PathBuf,
    sys: TSys,
}

pub type UnloadedContext<TSys = RealSysSync> = Context<TSys>;

impl<TSys: ContextSys> Context<TSys> {
    pub fn new(
        root_dir: &Path,
        inherit_env_vars: bool,
        root_marker: &str,
        env_files: Vec<String>,
        sys: TSys,
    ) -> Result<Self, ContextError> {
        Ok(Self {
            inherit_env_vars,
            env_files,
            workspace: get_workspace_configuration(root_dir, &sys)?,
            root_dir: root_dir.to_path_buf(),
            env_root_dir_marker: root_marker.to_string(),
            sys,
        })
    }
}

impl<TSys: ContextSys> Context<TSys> {
    pub fn sys(&self) -> &TSys {
        &self.sys
    }

    pub fn env_files(&self) -> &[String] {
        &self.env_files
    }

    pub fn env_root_dir_marker(&self) -> &str {
        &self.env_root_dir_marker
    }

    pub fn current_dir(&self) -> std::io::Result<PathBuf> {
        self.sys.env_current_dir()
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn workspace_configuration(&self) -> &WorkspaceConfiguration {
        &self.workspace
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub async fn into_loaded(
        self,
    ) -> Result<LoadedContext<TSys>, ContextError> {
        let start = std::time::Instant::now();
        let project_paths = ProjectDiscovery::new(
            self.root_dir(),
            self.workspace.projects.as_slice(),
        )
        .discover_project_files()
        .await?;

        let result = self.into_loaded_impl(project_paths).await;

        trace::info!(
            "{}",
            format!("Loaded context in {:?}", start.elapsed()).bold()
        );

        result
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub async fn into_loaded_with_walker<TDirWalker: DirWalker>(
        self,
        walker: &TDirWalker,
    ) -> Result<LoadedContext<TSys>, ContextError> {
        let start = std::time::Instant::now();
        let project_paths = ProjectDiscovery::new(
            self.root_dir(),
            self.workspace.projects.as_slice(),
        )
        .discover_project_files_with_walker(walker)
        .await?;

        let result = self.into_loaded_impl(project_paths).await;

        trace::info!(
            "{}",
            format!("Loaded context in {:?}", start.elapsed()).bold()
        );

        result
    }

    async fn into_loaded_impl(
        self,
        project_paths: Vec<DiscoveredPath>,
    ) -> Result<LoadedContext<TSys>, ContextError> {
        let project_paths = project_paths
            .into_iter()
            .filter_map(|p| match p {
                DiscoveredPath::Real { file } => Some(file),
                DiscoveredPath::Virtual { .. } => None,
            })
            .collect::<Vec<_>>();

        let project_configs =
            ProjectConfigLoader::<TSys>::new(&self.sys, self.root_dir())
                .load_project_configs(&project_paths)
                .await?;
        let mut xt_graph = ExtensionGraph::from_nodes(project_configs)?;
        let project_configs = xt_graph.get_or_process_all_nodes()?;

        let mut env_loader = self.create_env_loader();

        let extractions = ProjectDataExtractor::new(
            &self.root_dir,
            &mut env_loader,
            self.inherit_env_vars,
        )
        .extract_all(&project_configs, &project_paths, &xt_graph)?;

        // run validations
        ExtractedDataValidator::new(false).validate(&extractions)?;

        Ok(LoadedContext::new(env_loader, self, extractions))
    }

    pub fn create_env_loader(&self) -> EnvLoader<TSys> {
        let env_loader = EnvLoader::new(
            self.sys.clone(),
            PathBuf::from(&self.env_root_dir_marker),
            self.env_files.iter().map(|s| PathBuf::from(&s)).collect(),
        );
        env_loader
    }

    pub fn create_local_cache_store(&self) -> LocalTaskExecutionCacheStore {
        LocalTaskExecutionCacheStore::new(
            self.root_dir.join(CACHE_DIR),
            self.root_dir.clone(),
        )
    }
}

pub fn get_root_dir(sys: &impl ContextSys) -> Result<PathBuf, ContextError> {
    let current_dir = sys.env_current_dir()?;

    for p in current_dir.ancestors() {
        let workspace_files = constants::SUPPORTED_EXTENSIONS
            .iter()
            .map(|ext| constants::WORKSPACE_OMNI.replace("{ext}", ext));

        for workspace_file in workspace_files {
            let f = p.join(workspace_file);
            if sys.fs_exists(&f)? && sys.fs_is_file(&f)? {
                return Ok(p.to_path_buf());
            }
        }
    }

    Err(ContextErrorInner::FailedToFindWorkspaceConfiguration.into())
}

fn get_workspace_configuration(
    root_dir: &Path,
    sys: &impl ContextSys,
) -> Result<WorkspaceConfiguration, ContextError> {
    let workspace_files = constants::SUPPORTED_EXTENSIONS
        .iter()
        .map(|ext| constants::WORKSPACE_OMNI.replace("{ext}", ext));

    let mut ws_path = None;

    for workspace_file in workspace_files {
        let f = root_dir.join(workspace_file);
        if sys.fs_exists(&f)? && sys.fs_is_file(&f)? {
            ws_path = Some(f);
            break;
        }
    }

    let ws_path =
        ws_path.ok_or(ContextErrorInner::FailedToFindWorkspaceConfiguration)?;

    let f = sys.fs_read(&ws_path)?;
    let w =
        serde_yml::from_slice::<WorkspaceConfiguration>(&f).map_err(|e| {
            ContextErrorInner::FailedToLoadWorkspaceConfiguration(
                ws_path.clone(),
                e,
            )
        })?;

    Ok(w)
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ContextError {
    #[source]
    inner: ContextErrorInner,
    kind: ContextErrorKind,
}

impl ContextError {
    pub fn kind(&self) -> ContextErrorKind {
        self.kind
    }
}

impl<T: Into<ContextErrorInner>> From<T> for ContextError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(ContextErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum ContextErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to find workspace configuration")]
    FailedToFindWorkspaceConfiguration,

    #[error("failed to load workspace configuration: '{0}'")]
    FailedToLoadWorkspaceConfiguration(PathBuf, #[source] serde_yml::Error),

    #[error(transparent)]
    ProjectLoader(#[from] ProjectConfigLoaderError),

    #[error(transparent)]
    ProjectDataExtractor(#[from] ProjectDataExtractorError),

    #[error(transparent)]
    ProjectDiscovery(#[from] ProjectDiscoveryError),

    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error(transparent)]
    EnvLoader(#[from] EnvLoaderError),

    #[error(transparent)]
    ExtensionGraph(#[from] ExtensionGraphError),

    #[error(transparent)]
    ValidationError(#[from] ExtractedDataValidationErrors),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixture::*;
    use system_traits::*;

    fn ctx<TSys: ContextSys + 'static>(
        env: &str,
        root_dir: &Path,
        sys: TSys,
    ) -> Context<TSys> {
        Context::new(
            root_dir,
            false,
            "workspace.omni.yaml",
            vec![
                ".env".to_string(),
                ".env.local".to_string(),
                format!(".env.{env}"),
                format!(".env.{env}.local"),
            ],
            sys,
        )
        .expect("Can't create context")
    }

    #[tokio::test]
    async fn test_load_projects() {
        let (tmp, sys) = default_fixture();

        let loaded = ctx("testing", tmp.path(), sys)
            .into_loaded()
            .await
            .expect("can't load projects");

        let projects = loaded.projects();

        assert_eq!(projects.len(), 3, "Should be 3 projects");

        let project_1 = projects.iter().find(|p| p.name == "project-1");

        assert!(project_1.is_some(), "Can't find project-1");

        let project_2 = projects.iter().find(|p| p.name == "project-2");

        assert!(project_2.is_some(), "Can't find project-2");

        let project_3 = projects.iter().find(|p| p.name == "project-3");

        assert!(project_3.is_some(), "Can't find project-3");
    }

    #[tokio::test]
    async fn test_load_projects_with_duplicate_names() {
        let sys = real_sys();
        let tmp = tmp();
        let project4dir = tmp.path().join("nested").join("project-4");

        sys.fs_create_dir_all(&project4dir)
            .expect("Can't create project-4 dir");
        sys.fs_write(
            &project4dir.join("project.omni.yaml"),
            include_str!("../test_fixtures/project-1.omni.yaml"),
        )
        .expect("Can't write project config file");

        setup_fixture(tmp.path(), sys.clone());

        let ctx = ctx("testing", tmp.path(), sys);

        let loaded = ctx.into_loaded().await;
        let error = loaded.expect_err("should be an error");

        assert_eq!(
            error.kind(),
            ContextErrorKind::ValidationError,
            "should be a validation error"
        );
        assert!(
            error.to_string().contains("duplicate project name"),
            "should report duplicate project name"
        );
    }

    #[tokio::test]
    async fn test_project_extensions() {
        let (tmp, sys) = default_fixture();

        let ctx = ctx("testing", tmp.path(), sys.clone())
            .into_loaded()
            .await
            .expect("can't load context");

        let project_graph = ctx.get_project_graph().expect("Can't get graph");
        let project3 = project_graph
            .get_project_by_name("project-3")
            .expect("Can't get project-3");

        assert_eq!(project3.tasks.len(), 2, "Should be 2 tasks");
        assert_eq!(
            project3.tasks["from-base-1"].command,
            "echo \"from base-1\""
        );
        assert_eq!(
            project3.tasks["from-base-2"].command,
            "echo \"from base-2\""
        );
    }

    #[tokio::test]
    async fn test_loaded_environmental_variables() {
        let (tmp, sys) = default_fixture();

        let ctx = ctx("testing", tmp.path(), sys.clone())
            .into_loaded()
            .await
            .expect("can't load context");

        let project3dir = tmp.path().join("nested").join("project-3");
        let envs = ctx
            .get_cached_env_vars(&project3dir)
            .expect("can't get env vars");

        assert_eq!(envs["PROJECT_NAME"], "project-3");

        let project3dircanon = sys
            .fs_canonicalize(project3dir)
            .expect("can't canonicalize");

        let env_project3dircanon = sys
            .fs_canonicalize(Path::new(&envs["PROJECT_DIR"]))
            .expect("can't canonicalize");

        assert_eq!(env_project3dircanon, project3dircanon);
    }
}
