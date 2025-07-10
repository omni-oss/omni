use std::{
    collections::HashMap,
    env,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

use env_loader::EnvLoaderError;
use eyre::Context as _;
use globset::{Glob, GlobSetBuilder};
use system_traits::{
    EnvCurrentDir, FsCanonicalize, FsMetadata, FsRead, auto_impl,
    impls::RealSysSync,
};

use crate::{
    commands::CliArgs,
    configurations::{
        ProjectConfiguration, TaskConfiguration, WorkspaceConfiguration,
    },
    constants,
    core::{Project, Task},
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Context<TSys: ContextSys = RealSysSync> {
    envs_map: HashMap<String, String>,
    env_root_dir_marker: String,
    env_files: Vec<String>,
    projects: Option<Vec<Project>>,
    workspace: WorkspaceConfiguration,
    root_dir: PathBuf,
    sys: TSys,
}

#[auto_impl]
pub trait ContextSys:
    EnvCurrentDir + FsRead + FsMetadata + FsCanonicalize + Clone
{
}

impl<TSys: ContextSys> Context<TSys> {
    pub fn from_args_and_sys(
        cli: &CliArgs,
        sys: TSys,
    ) -> eyre::Result<Context<TSys>> {
        let envs_map = HashMap::new();

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

        let root_dir = get_root_dir()?;

        let ctx = Context {
            projects: None,
            envs_map,
            env_files,
            workspace: get_workspace_configuration(&root_dir)?,
            root_dir,
            env_root_dir_marker: cli
                .env_root_dir_marker
                .clone()
                .unwrap_or_else(|| {
                    constants::WORKSPACE_OMNI.replace("{ext}", "yaml")
                }),
            sys,
        };

        Ok(ctx)
    }

    pub fn get_env_var(&self, key: &str) -> Option<&str> {
        self.envs_map.get(key).map(|s| s.as_str())
    }

    pub fn set_env_var(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.envs_map.insert(key.into(), value.into());
    }

    pub fn remove_env_var(&mut self, key: &str) {
        self.envs_map.remove(key);
    }

    pub fn clear_env_vars(&mut self) {
        self.envs_map.clear();
    }

    pub fn get_all_env_vars(&self) -> &HashMap<String, String> {
        &self.envs_map
    }

    pub fn load_env_vars(
        &mut self,
        start_dir: &str,
    ) -> Result<(), EnvLoaderError> {
        self.envs_map.clear();
        let v = std::env::vars();

        let mut env_vars = HashMap::new();

        env_vars.extend(v);

        let config = env_loader::EnvConfig {
            root_file: Some(&self.env_root_dir_marker),
            start_dir: Some(start_dir),
            env_files: &self
                .env_files
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
            extra_envs: Some(&env_vars),
            matcher: None,
        };

        let env = env_loader::load(&config, self.sys.clone())?;
        self.envs_map.extend(env);

        Ok(())
    }

    pub fn load_env_vars_from_current_dir(
        &mut self,
    ) -> Result<(), EnvLoaderError> {
        let current_dir =
            std::env::current_dir().expect("Can't get current dir");

        self.load_env_vars(&current_dir.to_string_lossy())
    }

    pub fn reload_env_vars(
        &mut self,
        start_dir: &str,
    ) -> Result<(), EnvLoaderError> {
        self.clear_env_vars();
        self.load_env_vars(start_dir)
    }

    pub fn reload_env_vars_from_current_dir(
        &mut self,
    ) -> Result<(), EnvLoaderError> {
        self.clear_env_vars();
        self.load_env_vars_from_current_dir()
    }

    pub fn get_projects(&self) -> Option<&Vec<Project>> {
        self.projects.as_ref()
    }

    pub fn load_projects(&mut self) -> eyre::Result<&Vec<Project>> {
        let mut paths = vec![];

        let mut match_b = GlobSetBuilder::new();

        for p in &self.workspace.projects {
            match_b.add(Glob::new(
                format!("{}/{}", &self.root_dir.display(), p).as_str(),
            )?);
        }

        let matcher = match_b.build()?;

        for f in ignore::WalkBuilder::new(&self.root_dir)
            .add_custom_ignore_filename(constants::OMNI_IGNORE)
            .build()
        {
            let f = f?;

            if !f.path().is_dir() {
                continue;
            }

            let dir = f.path().display();
            let dir_str = dir.to_string();

            if matcher.is_match(&dir_str) && f.path().is_dir() {
                let project_files = constants::SUPPORTED_EXTENSIONS
                    .iter()
                    .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext));

                for project_file in project_files {
                    let p = f.path().join(&project_file);
                    if p.exists() && p.is_file() {
                        tracing::debug!("Found project directory: {}", dir);
                        paths.push((dir_str, f.path().join(&project_file)));
                        break;
                    }
                }
            }
        }

        let mut projects = vec![];

        for (dir, conf) in paths {
            let project = ProjectConfiguration::load(&conf as &Path)
                .wrap_err_with(|| {
                    format!(
                        "Failed to load project configuration file at {}",
                        conf.display()
                    )
                })?;

            let project_dependencies = project.dependencies.to_vec();
            projects.push(Project::new(
                project.name,
                PathBuf::from(dir),
                project
                    .dependencies
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                project
                    .tasks
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, v)| {
                        let mut mapped: Task = v.clone().into();

                        if let TaskConfiguration::LongForm {
                            merge_project_dependencies: merge_dependencies,
                            ..
                        } = v
                            && *merge_dependencies
                        {
                            mapped.dependencies.extend(
                                project_dependencies
                                    .iter()
                                    .cloned()
                                    .map(Into::into),
                            );
                        }

                        (k.clone(), mapped)
                    })
                    .collect(),
            ));
        }

        self.projects = Some(projects);

        Ok(self
            .projects
            .as_ref()
            .expect("Should be able to load projects"))
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn get_filtered_projects(
        &self,
        glob_filter: &str,
    ) -> eyre::Result<Vec<&Project>> {
        let glob = Glob::new(glob_filter)?;
        let matcher = glob.compile_matcher();
        let result = self
            .get_projects()
            .expect("Should be able to get projects after load");

        Ok(result
            .iter()
            .filter(|p| matcher.is_match(&p.name))
            .collect())
    }
}

fn get_root_dir() -> eyre::Result<PathBuf> {
    let current_dir = env::current_dir()?;

    for p in current_dir.ancestors() {
        let workspace_files = constants::SUPPORTED_EXTENSIONS
            .iter()
            .map(|ext| constants::WORKSPACE_OMNI.replace("{ext}", ext));

        for workspace_file in workspace_files {
            let f = p.join(workspace_file);
            if f.exists() && f.is_file() {
                return Ok(p.to_path_buf());
            }
        }
    }

    eyre::bail!("Could not find workspace configuration file");
}

fn get_workspace_configuration(
    root_dir: &Path,
) -> eyre::Result<WorkspaceConfiguration> {
    let workspace_files = constants::SUPPORTED_EXTENSIONS
        .iter()
        .map(|ext| constants::WORKSPACE_OMNI.replace("{ext}", ext));

    let mut ws_path = None;

    for workspace_file in workspace_files {
        let f = root_dir.join(workspace_file);
        if f.exists() && f.is_file() {
            ws_path = Some(f);
            break;
        }
    }

    let ws_path = ws_path.ok_or_else(|| {
        eyre::eyre!("Could not find workspace configuration file")
    })?;

    let f = OpenOptions::new().read(true).open(&ws_path)?;
    let w = serde_yml::from_reader::<_, WorkspaceConfiguration>(f)
        .wrap_err_with(|| {
            format!(
                "Failed to load workspace configuration '{}'",
                ws_path.to_string_lossy()
            )
        })?;

    Ok(w)
}
