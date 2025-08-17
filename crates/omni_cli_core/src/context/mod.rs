mod env_loader;

use config_utils::ListConfig;
use enum_map::enum_map;
pub(crate) use env_loader::{EnvLoader, GetVarsArgs};
use maps::UnorderedMap;
use merge::Merge;
use omni_cache::impls::LocalTaskExecutionCacheStore;
use omni_types::{OmniPath, Root};
use std::{
    borrow::Cow,
    collections::HashSet,
    path::{Path, PathBuf},
};
use trace::Level;

use ::env_loader::EnvLoaderError;
use dir_walker::{DirEntry as _, DirWalker};
use eyre::{Context as _, ContextCompat};
use globset::{Glob, GlobMatcher, GlobSetBuilder};
use omni_core::{ProjectGraph, TaskExecutionNode};
use system_traits::{
    EnvCurrentDir, EnvVar, EnvVars, FsCanonicalize, FsHardLinkAsync,
    FsMetadata, FsMetadataAsync, FsRead, auto_impl,
    impls::RealSys as RealSysSync,
};

use crate::{
    commands::CliArgs,
    configurations::{
        ExtensionGraph, ExtensionGraphNode, MetaConfiguration,
        ProjectConfiguration, TaskConfiguration, TaskOutputConfiguration,
        WorkspaceConfiguration,
    },
    constants::{self},
    core::{Project, Task},
    utils::env::EnvVarsMap,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheInfo {
    pub cache_execution: bool,
    pub key_defaults: bool,
    pub key_env_keys: Vec<String>,
    pub key_input_files: Vec<OmniPath>,
    pub cache_output_files: Vec<OmniPath>,
    pub cache_logs: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Context<TSys: ContextSys = RealSysSync> {
    env_loader: EnvLoader<TSys>,
    task_env_vars: UnorderedMap<String, EnvVarsMap>,
    cache_infos: UnorderedMap<String, CacheInfo>,
    task_meta_configs: UnorderedMap<String, MetaConfiguration>,
    project_meta_configs: UnorderedMap<String, MetaConfiguration>,
    env_root_dir_marker: String,
    env_files: Vec<String>,
    inherit_env_vars: bool,
    projects: Option<Vec<Project>>,
    workspace: WorkspaceConfiguration,
    root_dir: PathBuf,
    sys: TSys,
}

#[auto_impl]
pub trait ContextSys:
    EnvCurrentDir
    + FsRead
    + FsMetadata
    + FsMetadataAsync
    + FsCanonicalize
    + Clone
    + EnvVar
    + EnvVars
    + FsHardLinkAsync
    + Send
    + Sync
{
}

impl<TSys: ContextSys> Context<TSys> {
    pub fn new(
        root_dir: &Path,
        inherit_env_vars: bool,
        root_marker: &str,
        env_files: Vec<String>,
        sys: TSys,
    ) -> eyre::Result<Self> {
        Ok(Self {
            projects: None,
            inherit_env_vars,
            task_env_vars: maps::unordered_map!(),
            cache_infos: maps::unordered_map!(),
            task_meta_configs: maps::unordered_map!(),
            project_meta_configs: maps::unordered_map!(),
            env_loader: EnvLoader::new(
                sys.clone(),
                PathBuf::from(&root_marker),
                env_files
                    .iter()
                    .map(|s| Path::new(&s).to_path_buf())
                    .collect(),
            ),
            env_files,
            workspace: get_workspace_configuration(root_dir, &sys)?,
            root_dir: root_dir.to_path_buf(),
            env_root_dir_marker: root_marker.to_string(),
            sys,
        })
    }

    pub fn from_args_root_dir_and_sys(
        cli: &CliArgs,
        root_dir: impl AsRef<Path>,
        sys: TSys,
    ) -> eyre::Result<Context<TSys>> {
        let env = cli.env.as_deref().unwrap_or("development");
        let env_files = cli
            .env_file
            .iter()
            .map(|s| {
                if s.contains("{ENV}") {
                    s.replace("{ENV}", env)
                } else {
                    s.to_string()
                }
            })
            .collect::<Vec<_>>();

        let root_marker =
            cli.env_root_dir_marker.clone().unwrap_or_else(|| {
                constants::WORKSPACE_OMNI.replace("{ext}", "yaml")
            });
        let ctx = Context::new(
            root_dir.as_ref(),
            cli.inherit_env_vars,
            &root_marker,
            env_files,
            sys,
        )?;

        Ok(ctx)
    }

    pub fn from_args_and_sys(
        cli: &CliArgs,
        sys: TSys,
    ) -> eyre::Result<Context<TSys>> {
        let root_dir = get_root_dir(&sys)?;

        let ctx = Self::from_args_root_dir_and_sys(cli, root_dir, sys)?;

        Ok(ctx)
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

    pub fn get_project_graph(&self) -> eyre::Result<ProjectGraph> {
        let projects = self.get_projects().ok_or_else(|| {
            eyre::eyre!(
                "failed to get projects. Did you call load_projects first?"
            )
        })?;

        Ok(ProjectGraph::from_projects(projects.to_vec())?)
    }

    pub fn get_current_dir(&self) -> std::io::Result<PathBuf> {
        self.sys.env_current_dir()
    }

    pub fn get_env_vars(
        &mut self,
        args: Option<&GetVarsArgs>,
    ) -> Result<EnvVarsMap, EnvLoaderError> {
        let envs = self.env_loader.get(args.unwrap_or(&GetVarsArgs {
            ..Default::default()
        }))?;

        Ok(envs)
    }

    pub fn get_task_env_vars(
        &self,
        task: &TaskExecutionNode,
    ) -> eyre::Result<Cow<'_, EnvVarsMap>> {
        let cached = self.get_cached_env_vars(task.project_dir())?;

        if let Some(overrides) = self.task_env_vars.get(task.full_task_name()) {
            let mut cached = cached.clone();
            cached.extend(overrides.clone());
            Ok(Cow::Owned(cached))
        } else {
            Ok(Cow::Borrowed(cached))
        }
    }

    pub fn get_cached_env_vars(
        &self,
        path: &Path,
    ) -> eyre::Result<&EnvVarsMap> {
        let envs = self.env_loader.get_cached(path).ok_or_else(|| {
            eyre::eyre!(
                "env vars not found for path {} not found in cache",
                path.display()
            )
        })?;

        Ok(envs)
    }

    pub fn get_projects(&self) -> Option<&Vec<Project>> {
        self.projects.as_ref()
    }

    pub fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo> {
        self.cache_infos.get(&format!("{project_name}#{task_name}"))
    }

    pub fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.task_meta_configs
            .get(&format!("{project_name}#{task_name}"))
    }

    pub fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.project_meta_configs.get(project_name)
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub fn load_projects<TDirWalker: DirWalker>(
        &mut self,
        walker: &TDirWalker,
    ) -> eyre::Result<&Vec<Project>> {
        let start_time = std::time::SystemTime::now();

        let mut project_paths = vec![];

        let mut match_b = GlobSetBuilder::new();

        for p in &self.workspace.projects {
            match_b.add(Glob::new(
                format!("{}/{}", &self.root_dir.display(), p).as_str(),
            )?);
        }

        let matcher = match_b.build()?;

        let start_walk_time = std::time::SystemTime::now();

        let mut num_iterations = 0;

        let project_files: Vec<_> = constants::SUPPORTED_EXTENSIONS
            .iter()
            .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext))
            .collect();

        for f in walker
            .walk_dir(&[&self.root_dir])
            .map_err(|e| eyre::eyre!("failed to create walk dir: {e}"))?
        {
            num_iterations += 1;
            let f = f.map_err(|e| eyre::eyre!("failed to walk dir: {e}"))?;

            if !self.sys.fs_is_dir(f.path())? {
                continue;
            }

            let dir = self.sys.fs_canonicalize(f.path())?;
            let dir_str = dir.display().to_string();

            if matcher.is_match(&dir_str) {
                for project_file in &project_files {
                    let p = f.path().join(project_file);
                    if self.sys.fs_exists(&p)? && self.sys.fs_is_file(p)? {
                        trace::debug!("Found project directory: {:?}", dir);
                        project_paths.push(
                            self.sys.fs_canonicalize(dir.join(project_file))?,
                        );
                        break;
                    }
                }
            }
        }

        trace::debug!(
            "Found {} project directories in {} ms, walked {} items",
            project_paths.len(),
            start_walk_time.elapsed().unwrap_or_default().as_millis(),
            num_iterations
        );

        let mut project_configs = vec![];
        {
            let mut loaded = HashSet::new();
            let mut to_process = project_paths.clone();
            while let Some(conf) = to_process.pop() {
                let start_time = std::time::SystemTime::now();

                let mut project = ProjectConfiguration::load(
                    &conf as &Path,
                    self.sys.clone(),
                )
                .wrap_err_with(|| {
                    format!(
                        "failed to load project configuration file at {}",
                        conf.display()
                    )
                })?;

                let elapsed = start_time.elapsed().unwrap_or_default();
                trace::debug!(
                    project_configuration = ?project,
                    "loaded project configuration file {:?} in {} ms",
                    conf,
                    elapsed.as_millis()
                );

                project.file = conf.clone().into();
                let project_dir = conf.parent().expect("should have parent");
                project.dir = project_dir.into();
                loaded.insert(project.file.clone());

                let bases = enum_map! {
                    Root::Workspace => self.root_dir.as_ref(),
                    Root::Project => project_dir,
                };

                // resolve @project paths to the current project dir
                project.cache.key.files.iter_mut().for_each(|a| {
                    if a.is_project_rooted() {
                        a.resolve_in_place(&bases);
                    }
                });

                project.tasks.values_mut().for_each(|a| {
                    if let TaskConfiguration::LongForm(a) = a {
                        a.cache.key.files.iter_mut().for_each(|a| {
                            if a.is_project_rooted() {
                                a.resolve_in_place(&bases);
                            }
                        });

                        a.output.files.iter_mut().for_each(|a| {
                            if a.is_project_rooted() {
                                a.resolve_in_place(&bases);
                            }
                        });
                    }
                });

                for dep in &mut project.extends {
                    use omni_types::Root;

                    if dep.is_rooted() {
                        let roots = enum_map! {
                            Root::Workspace => self.root_dir.as_ref(),
                            Root::Project => project_dir,
                        };
                        dep.resolve_in_place(&roots);
                    }

                    *dep = self
                        .sys
                        .fs_canonicalize(project_dir.join(
                            dep.path().expect("path should be resolved"),
                        ))?
                        .into();

                    if !loaded.contains(dep) {
                        to_process.push(
                            dep.path()
                                .expect("path should be resolved")
                                .to_path_buf(),
                        );
                    }
                }

                project_configs.push(project);
            }
        }
        let mut xt_graph = ExtensionGraph::from_nodes(project_configs)?;
        let project_configs = xt_graph.get_or_process_all_nodes()?;

        let mut projects = vec![];

        let project_paths = project_paths
            .iter()
            .map(|p| p as &Path)
            .collect::<HashSet<_>>();

        let workspace_dir = self.root_dir.to_string_lossy().to_string();
        for project_config in project_configs.into_iter().filter(|config| {
            !config.base
                && project_paths.contains(
                    config.file.path().expect("path should be resolved"),
                )
        }) {
            trace::debug!(
                project_configuration = ?project_config,
                "processing project config: {}",
                project_config.name
            );

            let dir =
                project_config.dir.path().expect("path should be resolved");
            let mut extras = maps::map![
                "WORKSPACE_DIR".to_string() => workspace_dir.clone(),
                "PROJECT_NAME".to_string() => project_config.name.to_string(),
                "PROJECT_DIR".to_string() => dir.to_string_lossy().to_string(),
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
            let _loaded = self.get_env_vars(Some(&GetVarsArgs {
                start_dir: Some(dir),
                override_vars: Some(&extras),
                env_files: if env_files.is_empty() {
                    Some(&env_files[..])
                } else {
                    None
                },
                inherit_env_vars: self.inherit_env_vars,
            }))?;

            let project_cache = &project_config.cache;
            let meta_config = &project_config.meta;

            self.project_meta_configs
                .insert(project_config.name.clone(), meta_config.clone());

            for (name, task) in project_config.tasks.iter() {
                if let Some(env) = task.env()
                    && let Some(vars) = env.vars.as_ref()
                {
                    let key = format!("{}#{}", project_config.name, name);

                    self.task_env_vars.insert(key, vars.to_map_to_inner());
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

                let key_files = if cache.key.defaults {
                    let mut files = cache.key.files.clone();
                    let mut additional_files = xt_graph
                        .get_transitive_extendee_ids(project_config.id())?;

                    additional_files.push(project_config.id().clone());

                    files.merge(ListConfig::prepend(additional_files));
                    files.to_vec()
                } else {
                    cache.key.files.to_vec()
                };

                self.cache_infos.insert(
                    format!("{}#{}", project_config.name, name),
                    CacheInfo {
                        cache_execution: cache.enabled,
                        key_defaults: cache.key.defaults,
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

                self.task_meta_configs
                    .insert(format!("{}#{}", project_config.name, name), meta);
            }

            projects.push(Project::new(
                project_config.name,
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

        // check duplicate names
        let mut names = HashSet::new();
        for project in &projects {
            if names.contains(&project.name) {
                let projects = projects
                    .iter()
                    .filter(|p| p.name == project.name)
                    .map(|p| format!("  -> {}", p.dir.display()))
                    .collect::<Vec<_>>()
                    .join("\n");

                return Err(eyre::eyre!(
                    "Duplicate project name: {}\n\nProjects with same name:\n{}",
                    project.name,
                    projects
                ));
            }

            names.insert(project.name.clone());
        }

        self.projects = Some(projects);

        let elapsed = start_time.elapsed().unwrap_or_default();
        trace::info!(
            "Loaded {} projects in {:?}",
            self.projects.as_ref().map_or(0, |p| p.len()),
            elapsed
        );

        Ok(self
            .projects
            .as_ref()
            .expect("Should be able to load projects"))
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn get_filter_matcher(
        &self,
        glob_filter: &str,
    ) -> eyre::Result<GlobMatcher> {
        Ok(Glob::new(glob_filter)?.compile_matcher())
    }

    pub fn get_filtered_projects(
        &self,
        glob_filter: &str,
    ) -> eyre::Result<Vec<&Project>> {
        // short circuit if glob_filter is *, micro-optimization
        if glob_filter == "*" || glob_filter == "**" {
            return Ok(self
                .get_projects()
                .wrap_err("failed to get projects")?
                .iter()
                .collect());
        }

        let matcher = self.get_filter_matcher(glob_filter)?;
        let result = self
            .get_projects()
            .wrap_err("should be able to get projects after load")?;

        Ok(result
            .iter()
            .filter(|p| matcher.is_match(&p.name))
            .collect())
    }

    pub fn get_workspace_configuration(&self) -> &WorkspaceConfiguration {
        &self.workspace
    }

    pub fn create_local_cache_store(&self) -> LocalTaskExecutionCacheStore {
        LocalTaskExecutionCacheStore::new(
            self.root_dir.join(".omni/cache"),
            self.root_dir.clone(),
        )
    }
}

fn get_root_dir(sys: &impl ContextSys) -> eyre::Result<PathBuf> {
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

    eyre::bail!("Could not find workspace configuration file");
}

fn get_workspace_configuration(
    root_dir: &Path,
    sys: &impl ContextSys,
) -> eyre::Result<WorkspaceConfiguration> {
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

    let ws_path = ws_path.ok_or_else(|| {
        eyre::eyre!("Could not find workspace configuration file")
    })?;

    let f = sys.fs_read(&ws_path)?;
    let w = serde_yml::from_slice::<WorkspaceConfiguration>(&f).wrap_err_with(
        || {
            format!(
                "Failed to load workspace configuration: '{}'",
                ws_path.to_string_lossy()
            )
        },
    )?;

    Ok(w)
}

#[cfg(test)]
mod tests {
    use crate::tracer::TraceLevel;

    use super::*;
    use dir_walker::impls::{
        InMemoryDirEntry, InMemoryDirWalker, InMemoryMetadata,
    };
    use system_traits::impls::InMemorySys;
    use system_traits::*;

    fn create_sys() -> InMemorySys {
        let sys = InMemorySys::default();

        sys.fs_create_dir_all("/root/nested/project-1")
            .expect("Can't create project-1 dir");

        sys.fs_create_dir_all("/root/nested/project-2")
            .expect("Can't create project-2 dir");
        sys.fs_create_dir_all("/root/nested/project-3")
            .expect("Can't create project-3 dir");

        sys.fs_create_dir_all("/root/base")
            .expect("Can't create project-2 dir");

        sys.fs_write(
            "/root/.env",
            include_str!("../../test_fixtures/.env.root"),
        )
        .expect("Can't write root env file");
        sys.fs_write(
            "/root/.env.local",
            include_str!("../../test_fixtures/.env.root.local"),
        )
        .expect("Can't write root local env file");

        sys.fs_write(
            "/root/nested/.env",
            include_str!("../../test_fixtures/.env.nested"),
        )
        .expect("Can't write nested env file");
        sys.fs_write(
            "/root/nested/.env.local",
            include_str!("../../test_fixtures/.env.nested.local"),
        )
        .expect("Can't write nested local env file");

        sys.fs_write(
            "/root/nested/project-1/.env",
            include_str!("../../test_fixtures/.env.project-1"),
        )
        .expect("Can't write project env file");
        sys.fs_write(
            "/root/nested/project-1/.env.local",
            include_str!("../../test_fixtures/.env.project-1.local"),
        )
        .expect("Can't write project local env file");
        sys.fs_write(
            "/root/nested/project-1/project.omni.yaml",
            include_str!("../../test_fixtures/project-1.omni.yaml"),
        )
        .expect("Can't write project config file");

        sys.fs_write(
            "/root/nested/project-2/.env",
            include_str!("../../test_fixtures/.env.project-2"),
        )
        .expect("Can't write project env file");
        sys.fs_write(
            "/root/nested/project-2/.env.local",
            include_str!("../../test_fixtures/.env.project-2.local"),
        )
        .expect("Can't write project local env file");
        sys.fs_write(
            "/root/nested/project-2/project.omni.yaml",
            include_str!("../../test_fixtures/project-2.omni.yaml"),
        )
        .expect("Can't write project config file");
        sys.fs_write(
            "/root/nested/project-3/project.omni.yaml",
            include_str!("../../test_fixtures/project-3.omni.yaml"),
        )
        .expect("Can't write project config file");

        sys.fs_write(
            "/root/workspace.omni.yaml",
            include_str!("../../test_fixtures/workspace.omni.yaml"),
        )
        .expect("Can't write workspace config file");

        sys.fs_write(
            "/root/base/base-1.omni.yaml",
            include_str!("../../test_fixtures/base-1.omni.yaml"),
        )
        .expect("Can't write project config file");
        sys.fs_write(
            "/root/base/base-2.omni.yaml",
            include_str!("../../test_fixtures/base-2.omni.yaml"),
        )
        .expect("Can't write project config file");

        sys.env_set_current_dir("/root/nested/project-1")
            .expect("Can't set current dir");

        sys
    }

    fn create_ctx(env: &str, sys: Option<InMemorySys>) -> Context<InMemorySys> {
        let sys = sys.unwrap_or_else(create_sys);

        let cli = &CliArgs {
            env_root_dir_marker: None,
            env_file: vec![
                ".env".to_string(),
                ".env.local".to_string(),
                ".env.{ENV}".to_string(),
                ".env.{ENV}.local".to_string(),
            ],
            env: Some(String::from(env)),
            stdout_trace_level: TraceLevel::None,
            file_trace_level: TraceLevel::None,
            stderr_trace: false,
            file_trace_output: None,
            inherit_env_vars: false,
            no_inherit_env_vars: true,
            no_stderr_trace: false,
        };

        Context::from_args_and_sys(cli, sys).expect("Can't create context")
    }

    fn create_dir_entries() -> Vec<InMemoryDirEntry> {
        vec![
            InMemoryDirEntry::new(
                PathBuf::from("/root"),
                false,
                InMemoryMetadata::default(),
            ),
            InMemoryDirEntry::new(
                PathBuf::from("/root/base"),
                false,
                InMemoryMetadata::default(),
            ),
            InMemoryDirEntry::new(
                PathBuf::from("/root/nested"),
                false,
                InMemoryMetadata::default(),
            ),
            InMemoryDirEntry::new(
                PathBuf::from("/root/nested/project-1"),
                false,
                InMemoryMetadata::default(),
            ),
            InMemoryDirEntry::new(
                PathBuf::from("/root/nested/project-2"),
                false,
                InMemoryMetadata::default(),
            ),
            InMemoryDirEntry::new(
                PathBuf::from("/root/nested/project-3"),
                false,
                InMemoryMetadata::default(),
            ),
        ]
    }

    fn create_dir_walker(
        dir_entries: Option<Vec<InMemoryDirEntry>>,
    ) -> InMemoryDirWalker {
        let entries = dir_entries.unwrap_or_else(create_dir_entries);
        let walker = InMemoryDirWalker::new(entries);

        walker
    }

    #[test]
    pub fn test_load_env_vars() {
        let mut ctx = create_ctx("testing", None);

        let env = ctx.get_env_vars(None).expect("Can't load env vars");

        assert_eq!(
            env.get("SHARED_ENV").map(String::as_str),
            Some("root-local-nested-local-project-local")
        );
    }

    #[test]
    fn test_load_projects() {
        let mut ctx = create_ctx("testing", None);

        ctx.load_projects(&create_dir_walker(None))
            .expect("Can't load projects");

        let projects = ctx.get_projects().expect("Can't get projects");

        assert_eq!(projects.len(), 3, "Should be 3 projects");

        let project_1 = projects.iter().find(|p| p.name == "project-1");

        assert!(project_1.is_some(), "Can't find project-1");

        let project_2 = projects.iter().find(|p| p.name == "project-2");

        assert!(project_2.is_some(), "Can't find project-2");

        let project_3 = projects.iter().find(|p| p.name == "project-3");

        assert!(project_3.is_some(), "Can't find project-3");
    }

    #[test]
    fn test_load_projects_with_duplicate_names() {
        let sys = create_sys();
        sys.fs_create_dir_all("/root/nested/project-4")
            .expect("Can't create project-4 dir");
        sys.fs_write(
            "/root/nested/project-4/project.omni.yaml",
            include_str!("../../test_fixtures/project-1.omni.yaml"),
        )
        .expect("Can't write project config file");

        let mut ctx = create_ctx("testing", Some(sys));

        let mut dir_walker = create_dir_walker(None);

        dir_walker.add(InMemoryDirEntry::new(
            PathBuf::from("/root/nested/project-4"),
            false,
            InMemoryMetadata::default(),
        ));

        let projects = ctx.load_projects(&dir_walker);

        assert!(
            projects
                .expect_err("Should be an error")
                .to_string()
                .contains("Duplicate project name: project-1"),
            "Should report duplicate project name"
        );
    }

    #[test]
    fn test_get_project_graph() {
        let mut ctx = create_ctx("testing", None);

        ctx.load_projects(&create_dir_walker(None))
            .expect("Can't load projects");

        let project_graph = ctx.get_project_graph().expect("Can't get graph");

        assert_eq!(project_graph.count(), 3);
    }

    #[test]
    fn test_project_extensions() {
        let mut ctx = create_ctx("testing", None);

        ctx.load_projects(&create_dir_walker(None))
            .expect("Can't load projects");

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

    #[test]
    fn test_loaded_environmental_variables() {
        let mut ctx = create_ctx("testing", None);

        ctx.load_projects(&create_dir_walker(None))
            .expect("Can't load projects");

        let envs = ctx
            .get_cached_env_vars(&Path::new("/root/nested/project-3"))
            .expect("can't get env vars");

        println!("{envs:#?}");

        assert_eq!(envs["PROJECT_NAME"], "project-3");
        assert_eq!(envs["PROJECT_DIR"], "/root/nested/project-3");
    }
}
