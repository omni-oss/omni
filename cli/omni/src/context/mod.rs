use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

use dir_walker::{DirEntry as _, DirWalker};
use env_loader::EnvLoaderError;
use eyre::{Context as _, ContextCompat};
use globset::{Glob, GlobMatcher, GlobSetBuilder};
use omni_core::ProjectGraph;
use system_traits::{
    EnvCurrentDir, EnvVar, EnvVars, FsCanonicalize, FsMetadata, FsRead,
    auto_impl, impls::RealSys as RealSysSync,
};

use crate::{
    commands::CliArgs,
    configurations::{ProjectConfiguration, WorkspaceConfiguration},
    constants,
    core::{Project, Task},
    utils::env::{EnvVarsMap, EnvVarsMapOs, get_envs_at_start_dir},
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Context<TSys: ContextSys = RealSysSync> {
    envs_map: HashMap<String, String>,
    envs_map_os: HashMap<OsString, OsString>,
    env_root_dir_marker: String,
    env_files: Vec<String>,
    projects: Option<Vec<Project>>,
    workspace: WorkspaceConfiguration,
    root_dir: PathBuf,
    sys: TSys,
}

#[auto_impl]
pub trait ContextSys:
    EnvCurrentDir + FsRead + FsMetadata + FsCanonicalize + Clone + EnvVar + EnvVars
{
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

    pub fn from_args_and_sys(
        cli: &CliArgs,
        sys: TSys,
    ) -> eyre::Result<Context<TSys>> {
        let envs_map = HashMap::new();
        let envs_map_os = HashMap::new();

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

        let root_dir = get_root_dir(&sys)?;

        let ctx = Context {
            projects: None,
            envs_map,
            envs_map_os,
            env_files,
            workspace: get_workspace_configuration(&root_dir, &sys)?,
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

    pub fn get_project_graph(&self) -> eyre::Result<ProjectGraph> {
        let projects = self.get_projects().ok_or_else(|| {
            eyre::eyre!(
                "Failed to get projects. Did you run load_projects first?"
            )
        })?;

        Ok(ProjectGraph::from_projects(projects.to_vec())?)
    }

    pub fn get_env_var(&self, key: &str) -> Option<&str> {
        self.envs_map.get(key).map(|s| s.as_str())
    }

    pub fn get_current_dir(&self) -> std::io::Result<PathBuf> {
        self.sys.env_current_dir()
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
        self.envs_map_os.clear();
    }

    pub fn get_all_env_vars(&self) -> &HashMap<String, String> {
        &self.envs_map
    }

    pub fn get_all_env_vars_os(&self) -> &HashMap<OsString, OsString> {
        &self.envs_map_os
    }

    pub fn get_env_vars_at_start_dir(
        &self,
        start_dir: &str,
    ) -> Result<(EnvVarsMap, EnvVarsMapOs), EnvLoaderError> {
        get_envs_at_start_dir(
            start_dir,
            &self.env_root_dir_marker,
            self.env_files
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
            self.sys.clone(),
        )
    }

    pub fn load_env_vars(
        &mut self,
        start_dir: &str,
    ) -> Result<(), EnvLoaderError> {
        self.clear_env_vars();
        let (vars, vars_os) = self.get_env_vars_at_start_dir(start_dir)?;

        self.envs_map.extend(vars);

        self.envs_map_os.extend(vars_os);

        Ok(())
    }

    pub fn load_env_vars_from_current_dir(
        &mut self,
    ) -> Result<(), EnvLoaderError> {
        let current_dir =
            self.sys.env_current_dir().expect("Can't get current dir");

        self.load_env_vars(&current_dir.to_string_lossy())
    }

    pub fn get_projects(&self) -> Option<&Vec<Project>> {
        self.projects.as_ref()
    }

    pub fn load_projects<TDirWalker: DirWalker>(
        &mut self,
        walker: &TDirWalker,
    ) -> eyre::Result<&Vec<Project>> {
        let mut paths = vec![];

        let mut match_b = GlobSetBuilder::new();

        for p in &self.workspace.projects {
            match_b.add(Glob::new(
                format!("{}/{}", &self.root_dir.display(), p).as_str(),
            )?);
        }

        let matcher = match_b.build()?;

        for f in walker.walk_dir(&self.root_dir) {
            let f = f.map_err(|_| eyre::eyre!("Failed to walk dir"))?;

            if !self.sys.fs_is_dir(f.path())? {
                continue;
            }

            let dir = f.path().display();
            let dir_str = dir.to_string();

            if matcher.is_match(&dir_str) && self.sys.fs_is_dir(f.path())? {
                let project_files = constants::SUPPORTED_EXTENSIONS
                    .iter()
                    .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext));

                for project_file in project_files {
                    let p = f.path().join(&project_file);
                    if self.sys.fs_exists(&p)? && self.sys.fs_is_file(p)? {
                        trace::debug!("Found project directory: {}", dir);
                        paths.push((dir_str, f.path().join(&project_file)));
                        break;
                    }
                }
            }
        }

        let mut projects = vec![];

        for (dir, conf) in paths {
            let project =
                ProjectConfiguration::load(&conf as &Path, self.sys.clone())
                    .wrap_err_with(|| {
                        format!(
                            "Failed to load project configuration file at {}",
                            conf.display()
                        )
                    })?;

            projects.push(Project::new(
                project.name,
                PathBuf::from(dir),
                project.dependencies.to_vec(),
                project
                    .tasks
                    .unwrap_or_default()
                    .iter()
                    .map(|(task_name, v)| {
                        let mapped: Task = v.clone().into_task(task_name);

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
                .wrap_err("Failed to get projects")?
                .iter()
                .collect());
        }

        let matcher = self.get_filter_matcher(glob_filter)?;
        let result = self
            .get_projects()
            .expect("Should be able to get projects after load");

        Ok(result
            .iter()
            .filter(|p| matcher.is_match(&p.name))
            .collect())
    }

    pub fn get_workspace_configuration(&self) -> &WorkspaceConfiguration {
        &self.workspace
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
            "/root/workspace.omni.yaml",
            include_str!("../../test_fixtures/workspace.omni.yaml"),
        )
        .expect("Can't write workspace config file");

        sys.env_set_current_dir("/root/nested/project-1")
            .expect("Can't set current dir");

        sys
    }

    fn create_ctx(env: &str, sys: Option<InMemorySys>) -> Context<InMemorySys> {
        let sys = sys.unwrap_or_else(create_sys);

        let cli = &CliArgs {
            verbose: 0,
            env_root_dir_marker: None,
            env_file: vec![
                ".env".to_string(),
                ".env.local".to_string(),
                ".env.{ENV}".to_string(),
                ".env.{ENV}.local".to_string(),
            ],
            env: Some(String::from(env)),
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

        ctx.load_env_vars_from_current_dir()
            .expect("Can't load env vars");

        assert_eq!(
            ctx.get_env_var("SHARED_ENV"),
            Some("root-local-nested-local-project-local")
        );
    }

    #[test]
    fn test_load_projects() {
        let mut ctx = create_ctx("testing", None);

        ctx.load_projects(&create_dir_walker(None))
            .expect("Can't load projects");

        let projects = ctx.get_projects().expect("Can't get projects");

        assert_eq!(projects.len(), 2, "Should be 2 projects");

        let project_1 = projects.iter().find(|p| p.name == "project-1");

        assert!(project_1.is_some() == true, "Can't find project-1");

        let project_2 = projects.iter().find(|p| p.name == "project-2");

        assert!(project_2.is_some() == true, "Can't find project-2");
    }

    #[test]
    fn test_load_projects_with_duplicate_names() {
        let sys = create_sys();
        sys.fs_create_dir_all("/root/nested/project-3")
            .expect("Can't create project-3 dir");
        sys.fs_write(
            "/root/nested/project-3/project.omni.yaml",
            include_str!("../../test_fixtures/project-1.omni.yaml"),
        )
        .expect("Can't write project config file");

        let mut ctx = create_ctx("testing", Some(sys));

        let mut dir_walker = create_dir_walker(None);

        dir_walker.add(InMemoryDirEntry::new(
            PathBuf::from("/root/nested/project-3"),
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

        assert_eq!(project_graph.count(), 2);
    }
}
