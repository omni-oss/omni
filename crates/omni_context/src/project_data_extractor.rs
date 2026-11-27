use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use config_utils::{IntoInner as _, ListConfig};
use derive_new::new;
use env::{ExpansionError, expand_into_with_command_config};
use env_loader::EnvLoaderError;
use maps::{Map, OrderedMap, UnorderedMap};
use merge::Merge as _;
use omni_config_types::MergeSingleOrMany;
use omni_configurations::{
    FinalizedProject, LoadedLayer, MetaConfiguration, ProjectConfiguration,
    RedactPolicy, WorkspaceConfiguration,
};
use omni_core::{ExtensionGraph, ExtensionGraphError, Project, Task};
use omni_glob_config::MergeGlobConfig;
use omni_path_utils::{ProjectRootId, WorkspacePath};
use omni_task_context::{CacheInfo, EnvLayers};
use omni_task_output_logs::OutputLogsConfiguration;
use omni_types::{OmniPath, OmniPathError, TaskMap, task_label};
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tracing::Level;

use crate::{
    ConfigLocation, EnvLoader, EnvVarsMap, GetVarsArgs, build,
    env_loader::EnvCacheSys,
    project_config_entry::is_runnable_project,
    utils::{EnvVarsOsMap, vars_os},
};

/// Config files that seed a project's default cache key inputs: the file
/// itself and everything it transitively extends. A virtual project has no
/// file, so it contributes none and its logical key never becomes an input.
fn config_file_inputs(
    location: &ConfigLocation,
    extension_graph: &ExtensionGraph<LoadedLayer>,
) -> Result<Vec<OmniPath>, ExtensionGraphError> {
    let Some(file) = location.file_path() else {
        return Ok(vec![]);
    };
    let id = OmniPath::from(file.to_path_buf());
    let mut files = extension_graph.get_transitive_extendee_ids(&id)?;
    files.push(id);
    Ok(files)
}

struct RunnableProjectRoots<'a>(
    // None marks an ambiguous root; no insertion order can choose its project.
    UnorderedMap<ProjectRootId, Option<&'a FinalizedProject>>,
);

impl<'a> RunnableProjectRoots<'a> {
    fn new(
        workspace_root: &Path,
        projects: impl Iterator<Item = &'a FinalizedProject>,
    ) -> Self {
        let mut roots = UnorderedMap::default();
        for project in projects {
            let dir = project.dir.path().expect("path should be resolved");
            // Unrepresentable native paths cannot be named by a ProjectRootId.
            // They remain available through ordinary project-name references.
            let Ok(path) = WorkspacePath::from_os_path(workspace_root, dir)
            else {
                continue;
            };
            roots
                .entry(ProjectRootId::new(path))
                .and_modify(|entry| *entry = None)
                .or_insert(Some(project));
        }
        Self(roots)
    }

    fn resolve(
        &self,
        root: &ProjectRootId,
        task: &str,
    ) -> Result<String, &'static str> {
        match self.0.get(root) {
            None => Err("no runnable project has this root"),
            Some(None) => Err("multiple runnable projects have this root"),
            Some(Some(project))
                if !project.config.tasks.as_map().contains_key(task)
                    && project.aliases.resolve(task).is_none() =>
            {
                Err("the runnable project has no task or alias with this name")
            }
            Some(Some(project)) => Ok(project.config.name.clone()),
        }
    }
}

fn lower_tasks(
    config: &ProjectConfiguration,
    roots: &RunnableProjectRoots<'_>,
) -> Result<OrderedMap<String, Task>, ProjectDataExtractorErrorInner> {
    config
        .tasks
        .iter()
        .map(|(task_name, task)| {
            let lowered = task.clone().into_long_form(task_name).into_task(
                |field, root, target, origin| {
                    roots.resolve(root, target).map_err(|reason| {
                        ProjectDataExtractorErrorInner::InvalidTaskRootReference {
                            field,
                            location: task_label(&config.name, task_name),
                            contributor: origin.to_string().into_boxed_str(),
                            root: root.to_string(),
                            task: target.to_owned(),
                            reason,
                        }
                    })
                },
            )?;
            Ok((task_name.clone(), lowered))
        })
        .collect()
}

#[derive(new)]
pub struct ProjectDataExtractor<'a, TSys: EnvCacheSys> {
    sys: &'a TSys,
    root_dir: &'a Path,
    env_loader: &'a mut EnvLoader<TSys>,
    inherit_env_vars: bool,
    workspace_configuration: &'a WorkspaceConfiguration,
    #[new(default)]
    definitions_only: bool,
}

impl<'a, TSys: EnvCacheSys> ProjectDataExtractor<'a, TSys> {
    pub(crate) fn definitions_only(mut self) -> Self {
        self.definitions_only = true;
        self
    }

    fn get_ws_vars_vars(
        &self,
    ) -> Result<(EnvVarsMap, EnvVarsOsMap), ProjectDataExtractorError> {
        if self.definitions_only
            || self.workspace_configuration.env.vars.is_empty()
        {
            return Ok((EnvVarsMap::default(), EnvVarsOsMap::default()));
        }

        let base_vars = if self.inherit_env_vars {
            self.sys.env_vars().collect::<Map<_, _>>()
        } else {
            Map::default()
        };

        let base_vars_os = vars_os(&base_vars);

        let cfg = self.env_loader.command_config(self.root_dir, &base_vars_os);

        let mut ws_vars = self.workspace_configuration.env.vars.clone();
        expand_into_with_command_config(&mut ws_vars, &base_vars, &cfg)?;

        let ws_vars_os = vars_os(&ws_vars);

        Ok((ws_vars, ws_vars_os))
    }

    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub fn extract_all(
        &mut self,
        project_configs: &[FinalizedProject],
        project_paths: &[PathBuf],
        extension_graph: &ExtensionGraph<LoadedLayer>,
    ) -> Result<ProjectDataExtractions, ProjectDataExtractorError> {
        let mut projects = vec![];
        let mut project_meta_configs = maps::unordered_map![];
        let mut task_meta_configs = TaskMap::new();
        let mut task_env_var_overrides = TaskMap::new();
        let mut cache_infos = TaskMap::new();
        let mut cache_enabled_files: CacheEnabledFiles = maps::unordered_map![];
        let mut output_logs_configs = TaskMap::new();
        let mut redact_policies = TaskMap::new();

        let project_paths = project_paths
            .iter()
            .map(|p| p as &Path)
            .collect::<UnorderedSet<_>>();

        let root_dir = self.root_dir.to_string_lossy().to_string();

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
            .filter(|p| is_runnable_project(p, &project_paths));
        // Lower every task before any environment is loaded, so an invalid
        // reference fails without running command substitutions.
        let project_roots =
            RunnableProjectRoots::new(self.root_dir, filtered.clone());
        let lowered_tasks = filtered
            .clone()
            .map(|finalized| lower_tasks(&finalized.config, &project_roots))
            .collect::<Result<Vec<_>, _>>()?;
        let (ws_vars, ws_vars_os) = self.get_ws_vars_vars()?;

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

        for (finalized, tasks) in filtered.zip(lowered_tasks) {
            let project_config = &finalized.config;
            tracing::debug!(
                project_configuration = ?project_config,
                "processing_project_config",
            );

            let dir = finalized.dir.path().expect("path should be resolved");

            let mut project_extras = maps::map![
                "PROJECT_NAME".to_string() => project_config.name.to_string(),
                "PROJECT_DIR".to_string() => dir.to_string_lossy().to_string(),
            ];

            project_extras.extend(ws_extras.clone());

            let overrides = &project_config.env.vars;
            if !overrides.as_map().is_empty() {
                project_extras.extend(overrides.to_map_to_inner());
                if !ws_vars_os.is_empty() {
                    let cfg = self
                        .env_loader
                        .command_config(finalized.dir.path()?, &ws_vars_os);
                    expand_into_with_command_config(
                        &mut project_extras,
                        &self.workspace_configuration.env.vars,
                        &cfg,
                    )?;

                    project_extras.extend(ws_vars.clone());
                }
            } else if !ws_vars.is_empty() {
                project_extras.extend(ws_vars.clone());
            }

            // load the env vars for the project
            if !self.definitions_only {
                self.env_loader.get(&GetVarsArgs {
                    start_dir: Some(dir),
                    project_env_var_overrides: Some(&project_extras),
                    inherit_env_vars: self.inherit_env_vars,
                })?;
            }

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
                let project_name = project_config.name.as_str();
                if let Some(env) = task.env()
                    && let Some(vars) = env.vars.as_ref()
                {
                    task_env_var_overrides.insert(
                        project_name,
                        name,
                        vars.to_map_to_inner(),
                    );
                }

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
                    output_logs_configs.insert(project_name, name, output_logs);
                }

                let cache = finalized
                    .task_cache
                    .get(name)
                    .expect("every task has an effective cache");

                let enabled = *finalized
                    .task_cache_enabled
                    .get(name)
                    .expect("every task has an effective cache enablement");
                if let Some(file) = finalized.task_cache_enabled_file.get(name)
                {
                    cache_enabled_files
                        .entry(project_config.name.clone())
                        .or_default()
                        .insert(name.to_owned(), Arc::clone(file));
                }

                let use_defaults = cache.key.defaults.unwrap_or(true);

                let key_input_files = if use_defaults {
                    let mut files = cache.key.files.clone();
                    let location = match &finalized.file {
                        Some(file) => Some(ConfigLocation::File(
                            file.path()?.to_path_buf(),
                        )),
                        None => None,
                    };
                    let additional_files = match &location {
                        Some(location) => {
                            config_file_inputs(location, extension_graph)?
                        }
                        None => vec![],
                    };

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
                    project_name,
                    name,
                    CacheInfo {
                        cache_enabled: enabled.value,
                        cache_enabled_source: enabled.source,
                        cache_remote: cache
                            .remote
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
                        env: EnvLayers {
                            workspace: self
                                .workspace_configuration
                                .env
                                .vars
                                .clone(),
                            project: project_config.env.vars.to_map_to_inner(),
                            task: task
                                .env()
                                .and_then(|e| e.vars.as_ref())
                                .map(|v| v.to_map_to_inner())
                                .unwrap_or_default(),
                        },
                    },
                );

                let task_meta = finalized
                    .task_meta
                    .get(name)
                    .expect("every task has an effective meta")
                    .clone();

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
                    project_name,
                    name,
                    RedactPolicy::resolve(&levels),
                );

                task_meta_configs.insert(project_name, name, task_meta);
            }

            projects.push(
                Project::new(
                    project_config.name.clone(),
                    dir.to_path_buf(),
                    project_config.dependencies.to_vec_inner(),
                    tasks,
                )
                .with_aliases(finalized.aliases.clone()),
            );
        }

        let mut extracted = ProjectDataExtractions::new(
            projects,
            cache_infos,
            task_env_var_overrides,
            project_meta_configs,
            task_meta_configs,
            output_logs_configs,
            redact_policies,
        );
        extracted.cache_enabled_files = cache_enabled_files;
        Ok(extracted)
    }
}

type CacheEnabledFiles =
    UnorderedMap<String, UnorderedMap<String, Arc<OmniPath>>>;

#[derive(Debug, Clone, PartialEq, new, Serialize, Deserialize)]
pub struct ProjectDataExtractions {
    pub projects: Vec<Project>,
    pub cache_infos: TaskMap<CacheInfo>,
    #[new(default)]
    #[serde(skip)]
    pub(crate) cache_enabled_files: CacheEnabledFiles,
    pub task_env_var_overrides: TaskMap<Map<String, String>>,
    pub project_meta_configs: UnorderedMap<String, MetaConfiguration>,
    pub task_meta_configs: TaskMap<MetaConfiguration>,
    pub output_logs_configs: TaskMap<OutputLogsConfiguration>,
    /// Resolved redaction policy per `(project, task)`. Display only: never
    /// hashed and never part of cache keys.
    #[serde(default)]
    pub redact_policies: TaskMap<RedactPolicy>,
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

    /// An internal task reference did not resolve in the final runnable projects.
    #[error(
        "invalid task reference in `{field}` of `{location}` to `{task}` at project root `{root}` (contributed by {contributor}): {reason}"
    )]
    #[diagnostic(code = "workspace.invalid_task_root_reference")]
    InvalidTaskRootReference {
        #[diagnostic(detail)]
        field: &'static str,
        #[diagnostic(detail)]
        location: String,
        #[diagnostic(detail)]
        contributor: Box<str>,
        #[diagnostic(detail)]
        root: String,
        #[diagnostic(detail)]
        task: String,
        #[diagnostic(detail)]
        reason: &'static str,
    },

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalProjectKey;
    use omni_configurations::{
        TaskDependencyConfiguration, TaskLayer, TaskReferenceOrigin,
    };
    use omni_core::TaskDependency;
    use system_traits::impls::RealSys;

    fn layer(file: &str, extends: &[&str]) -> LoadedLayer {
        let mut l = LoadedLayer::from_file(
            PathBuf::from(file).into(),
            PathBuf::from(file).parent().unwrap().to_path_buf().into(),
            Default::default(),
        );
        if !extends.is_empty() {
            l.update_layer(|layer| {
                layer.extends =
                    Some(omni_configurations::types::SingleOrMany::Many(
                        extends
                            .iter()
                            .map(|e| OmniPath::from(PathBuf::from(e)))
                            .collect(),
                    ));
            });
        }
        l
    }

    fn graph() -> ExtensionGraph<LoadedLayer> {
        ExtensionGraph::from_nodes(vec![
            layer("/w/lib/base.yaml", &[]),
            layer("/w/app/project.omni.yaml", &["/w/lib/base.yaml"]),
        ])
        .unwrap()
    }

    #[test]
    fn a_file_project_seeds_inputs_with_its_closure_and_itself() {
        let inputs = config_file_inputs(
            &ConfigLocation::File(PathBuf::from("/w/app/project.omni.yaml")),
            &graph(),
        )
        .unwrap();

        assert_eq!(
            inputs.last(),
            Some(&OmniPath::from(PathBuf::from("/w/app/project.omni.yaml")))
        );
        assert!(
            inputs.contains(&OmniPath::from(PathBuf::from("/w/lib/base.yaml")))
        );
        // The closure already includes the file itself; kept as before.
        assert_eq!(inputs.len(), 3);
    }

    #[test]
    fn a_virtual_project_contributes_no_file_inputs() {
        let key = LogicalProjectKey::new(ProjectRootId::new(
            WorkspacePath::parse("app").unwrap(),
        ));

        // Not even a missing graph node is consulted: no id is ever built.
        let inputs = config_file_inputs(
            &ConfigLocation::Virtual(key),
            &ExtensionGraph::new(),
        )
        .unwrap();

        assert!(inputs.is_empty());
    }

    const EXTENDS_TEMPLATE: &str = r#"{"extends":"template"}"#;

    fn root_reference_fixture(
        target_layers: &[(&str, &str, bool)],
        run: &str,
        later_layer: Option<&str>,
    ) -> Result<ProjectDataExtractions, ProjectDataExtractorError> {
        root_reference_fixture_with_workspace(
            target_layers,
            run,
            later_layer,
            r#"{"projects":["**"]}"#,
        )
    }

    fn root_reference_fixture_with_workspace(
        target_layers: &[(&str, &str, bool)],
        run: &str,
        later_layer: Option<&str>,
        workspace: &str,
    ) -> Result<ProjectDataExtractions, ProjectDataExtractorError> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let configured_layer = |dir: &str, file: &str, json: &str| {
            let dir = root.join(dir);
            std::fs::create_dir_all(&dir).unwrap();
            LoadedLayer::from_file(
                dir.join(file).into(),
                dir.into(),
                serde_json::from_str(json).unwrap(),
            )
        };
        let mut app = configured_layer(
            "app",
            "base.omni.yaml",
            r#"{"tasks":{"template":{"base":true}}}"#,
        );
        let origin = TaskReferenceOrigin {
            task: "template".into(),
            file: Some(app.file().path().unwrap().to_owned()),
        };
        let project_root = ProjectRootId::new(
            WorkspacePath::parse("packages/shared").unwrap(),
        );
        let reference = |task: &str| TaskDependencyConfiguration::ProjectRoot {
            project_root: project_root.clone(),
            task: task.into(),
            origin: origin.clone(),
        };
        app.update_layer(|layer| {
            let TaskLayer::LongForm(template) =
                &mut layer.tasks.as_mut().unwrap().as_map_mut()["template"]
            else {
                panic!("expected a normal task template");
            };
            template.dependencies =
                Some(ListConfig::append(vec![reference("build")]));
            template.with = Some(ListConfig::append(vec![reference("watch")]));
        });
        app.merge(configured_layer(
            "app",
            "project.omni.yaml",
            &format!(r#"{{"name":"app","tasks":{{"run":{run}}}}}"#),
        ));
        if let Some(later_layer) = later_layer {
            app.merge(configured_layer(
                "app",
                "project.omni.yaml",
                later_layer,
            ));
        }

        let mut discovered = vec![app.file().path().unwrap().to_owned()];
        let mut layers = vec![app];
        for &(file, json, is_discovered) in target_layers {
            let target = configured_layer("packages/shared", file, json);
            if is_discovered {
                discovered.push(target.file().path().unwrap().to_owned());
            }
            layers.push(target);
        }
        let graph = ExtensionGraph::from_nodes(layers.clone()).unwrap();
        let finalized = layers
            .into_iter()
            .map(|layer| layer.finalize_composed().unwrap())
            .collect::<Vec<_>>();
        let workspace = serde_json::from_str(workspace).unwrap();
        let mut env_loader = EnvLoader::new(RealSys, root.to_owned(), vec![]);
        ProjectDataExtractor::new(
            &RealSys,
            root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&finalized, &discovered, &graph)
    }

    #[test]
    fn inherited_root_dependencies_and_with_follow_the_final_project_name() {
        for name in ["shared", "renamed-shared"] {
            let json = format!(
                r#"{{"name":"{name}","tasks":{{"build":"echo","watch":"echo"}}}}"#
            );
            let data = root_reference_fixture(
                &[("project.omni.yaml", &json, true)],
                EXTENDS_TEMPLATE,
                None,
            )
            .unwrap();
            let run = &data.projects[0].tasks["run"];
            let named = |task: &str| TaskDependency::ExplicitProject {
                project: name.into(),
                task: task.into(),
            };
            assert_eq!(run.dependencies, vec![named("build")]);
            assert_eq!(run.siblings, vec![named("watch")]);
        }
    }

    #[test]
    fn replaced_root_references_are_not_looked_up() {
        let data = root_reference_fixture(
            &[],
            EXTENDS_TEMPLATE,
            Some(
                r#"{"tasks":{"run":{"dependencies":{"replace":[]},"with":{"replace":[]}}}}"#,
            ),
        )
        .unwrap();
        let run = &data.projects[0].tasks["run"];
        assert!(run.dependencies.is_empty());
        assert!(run.siblings.is_empty());
    }

    #[test]
    fn a_later_append_does_not_bring_back_a_replaced_root_reference() {
        let data = root_reference_fixture(
            &[],
            r#"{"extends":"template","dependencies":{"replace":[]},"with":{"replace":[]}}"#,
            Some(
                r#"{"tasks":{"run":{"dependencies":{"append":["lint"]},"with":{"append":["format"]}}}}"#,
            ),
        )
        .unwrap();
        let run = &data.projects[0].tasks["run"];
        let own = |task: &str| TaskDependency::Own { task: task.into() };
        assert_eq!(run.dependencies, vec![own("lint")]);
        assert_eq!(run.siblings, vec![own("format")]);
    }

    #[test]
    fn surviving_root_references_require_a_runnable_target_task() {
        let missing_task = root_reference_fixture(
            &[(
                "project.omni.yaml",
                r#"{"name":"renamed","tasks":{"build":"echo","watch":{"base":true}}}"#,
                true,
            )],
            EXTENDS_TEMPLATE,
            None,
        )
        .unwrap_err();
        let message = missing_task.to_string();
        for detail in [
            "app#run",
            "`with`",
            "watch",
            "packages/shared",
            "no task",
            "contributed by task `template`",
            "base.omni.yaml",
        ] {
            assert!(message.contains(detail), "{message}");
        }

        for targets in [
            &[(
                "project.omni.yaml",
                r#"{"name":"shared","base":true}"#,
                true,
            )][..],
            &[("project.omni.yaml", r#"{"name":"shared"}"#, false)][..],
            &[],
        ] {
            let error = root_reference_fixture(targets, EXTENDS_TEMPLATE, None)
                .unwrap_err();
            assert!(
                error.to_string().contains("no runnable project"),
                "{error}"
            );
        }
    }

    #[test]
    fn invalid_root_is_rejected_before_workspace_command_expansion() {
        let workspace = r#"{"projects":["**"],"env":{"vars":{"PROBE":"$(omni-f03-nonexistent-expansion-probe)"}}}"#;
        let check = |targets: &[(&str, &str, bool)]| {
            root_reference_fixture_with_workspace(
                targets,
                EXTENDS_TEMPLATE,
                None,
                workspace,
            )
            .unwrap_err()
            .kind()
        };
        assert_eq!(
            check(&[]),
            ProjectDataExtractorErrorKind::InvalidTaskRootReference
        );
        // With valid references the workspace command does reach expansion.
        assert_eq!(
            check(&[(
                "project.omni.yaml",
                r#"{"name":"shared","tasks":{"build":"echo","watch":"echo"}}"#,
                true,
            )]),
            ProjectDataExtractorErrorKind::CommandExpansion,
        );
    }

    #[test]
    fn a_root_reference_does_not_choose_between_multiple_runnable_projects() {
        let error = root_reference_fixture(
            &[
                (
                    "first.omni.yaml",
                    r#"{"name":"first","tasks":{"build":"echo"}}"#,
                    true,
                ),
                (
                    "second.omni.yaml",
                    r#"{"name":"second","tasks":{"build":"echo"}}"#,
                    true,
                ),
            ],
            EXTENDS_TEMPLATE,
            None,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("multiple runnable projects"),
            "{error}"
        );
    }

    #[test]
    fn lowered_root_references_reach_the_real_task_graph_and_its_cycle_check() {
        let graph = |back_edge: &str| {
            let shared = format!(
                r#"{{"name":"shared","tasks":{{"build":{{"exec":"echo"{back_edge}}},"watch":"echo"}}}}"#
            );
            let data = root_reference_fixture(
                &[("project.omni.yaml", &shared, true)],
                EXTENDS_TEMPLATE,
                None,
            )
            .unwrap();
            omni_core::ProjectGraph::from_projects(data.projects)
                .unwrap()
                .get_task_execution_graph()
        };

        let acyclic =
            graph("").expect("a lowered reference is an ordinary edge");
        assert_eq!(acyclic.count(), 3);

        let cyclic = graph(r#","dependencies":["app#run"]"#)
            .err()
            .expect("a cycle closed through a root reference");
        assert_eq!(
            cyclic.kind(),
            omni_core::TaskExecutionGraphErrorKind::CycleDetected
        );
    }

    /// F05.1: the origin of each task's `cache.enabled` reaches `CacheInfo`, and
    /// the value is exactly what the extractor computed before origins existed.
    #[tokio::test]
    async fn extraction_carries_the_origin_of_cache_enablement() {
        use omni_configurations::{CacheEnabledSource, ProjectLayer};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let file = root.join("app/project.omni.yaml");
        let file = file.to_str().unwrap();
        std::fs::write(
            file,
            "name: app\ncache:\n  enabled: false\ntasks:\n  inherits: echo\n  own:\n    exec: x\n    cache:\n      enabled: true\n",
        )
        .unwrap();
        let layer = layer(file, &[]).with_layer(
            ProjectLayer::load(Path::new(file), &RealSys).await.unwrap(),
        );
        let graph = ExtensionGraph::from_nodes(vec![layer.clone()]).unwrap();
        let finalized = layer.finalize_composed().unwrap();

        let workspace: WorkspaceConfiguration =
            serde_json::from_str(r#"{"projects":["**"]}"#)
                .expect("a minimal workspace is valid");
        let mut env_loader = EnvLoader::new(RealSys, root.clone(), vec![]);
        let extracted = ProjectDataExtractor::new(
            &RealSys,
            &root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&[finalized], &[PathBuf::from(file)], &graph)
        .unwrap();
        let infos = &extracted.cache_infos;

        let inherits = infos.get("app", "inherits").unwrap();
        assert!(!inherits.cache_enabled);
        assert_eq!(
            inherits.cache_enabled_source,
            CacheEnabledSource::ProjectExplicit
        );
        let own = infos.get("app", "own").unwrap();
        assert!(own.cache_enabled);
        assert_eq!(own.cache_enabled_source, CacheEnabledSource::TaskExplicit);
        for task in ["inherits", "own"] {
            assert_eq!(
                extracted.cache_enabled_files["app"][task].path().unwrap(),
                Path::new(file)
            );
        }
        assert!(
            serde_json::to_value(extracted)
                .unwrap()
                .get("cache_enabled_files")
                .is_none()
        );
    }

    /// The extractor reads the effective task `meta` computed at finalization.
    /// Merging the project `meta` with the task's merged `meta` here instead
    /// would bring back the entry that the earlier declaration replaced.
    #[test]
    fn extraction_reads_the_effective_task_meta() {
        use omni_configurations::ProjectLayer;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let file = root.join("app/project.omni.yaml");
        let file = file.to_str().unwrap();
        let declare = |json: &str| {
            layer(file, &[])
                .with_layer(serde_json::from_str::<ProjectLayer>(json).unwrap())
        };
        let mut composed = declare(
            r#"{"name":"app","meta":{"tags":["project"]},"tasks":{
                "plain":"echo",
                "own":{"exec":"x","meta":{"tags":{"replace":[]}}}}}"#,
        );
        composed.merge(declare(
            r#"{"tasks":{"own":{"meta":{"tags":{"append":["task"]}}}}}"#,
        ));
        let graph = ExtensionGraph::from_nodes(vec![composed.clone()]).unwrap();
        let finalized = composed.finalize_composed().unwrap();

        let workspace: WorkspaceConfiguration =
            serde_json::from_str(r#"{"projects":["**"]}"#)
                .expect("a minimal workspace is valid");
        let mut env_loader = EnvLoader::new(RealSys, root.clone(), vec![]);
        let data = ProjectDataExtractor::new(
            &RealSys,
            &root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&[finalized], &[PathBuf::from(file)], &graph)
        .unwrap();

        let tags = |task: &str| {
            data.task_meta_configs.get("app", task).unwrap().0.as_map()["tags"]
                .clone()
                .into_json()
        };
        assert_eq!(tags("own"), serde_json::json!(["task"]));
        assert_eq!(tags("plain"), serde_json::json!(["project"]));
    }

    /// Environment the loader resolved for project `app` after extraction.
    fn resolved_env(
        workspace_env: &str,
        project_env: Option<&str>,
        at_root: bool,
        dotenv: Option<&str>,
        definitions_only: bool,
    ) -> Option<std::sync::Arc<crate::EnvVarsMap>> {
        use omni_configurations::ProjectLayer;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let project_dir = if at_root {
            root.clone()
        } else {
            root.join("app")
        };
        std::fs::create_dir_all(&project_dir).unwrap();
        if let Some(dotenv) = dotenv {
            std::fs::write(project_dir.join(".env"), dotenv).unwrap();
        }
        let file = project_dir.join("project.omni.yaml");
        let env = project_env
            .map(|vars| format!(r#","env":{{"vars":{vars}}}"#))
            .unwrap_or_default();
        let json = format!(r#"{{"name":"app","tasks":{{"t":"x"}}{env}}}"#);
        let layer = layer(file.to_str().unwrap(), &[])
            .with_layer(serde_json::from_str::<ProjectLayer>(&json).unwrap());
        let graph = ExtensionGraph::from_nodes(vec![layer.clone()]).unwrap();
        let finalized = layer.finalize_composed().unwrap();
        let workspace: WorkspaceConfiguration = serde_json::from_str(&format!(
            r#"{{"projects":["**"],"env":{{"vars":{workspace_env}}}}}"#
        ))
        .unwrap();
        let mut env_loader =
            EnvLoader::new(RealSys, root.clone(), vec![PathBuf::from(".env")]);
        let extractor = ProjectDataExtractor::new(
            &RealSys,
            &root,
            &mut env_loader,
            false,
            &workspace,
        );
        let mut extractor = if definitions_only {
            extractor.definitions_only()
        } else {
            extractor
        };
        extractor
            .extract_all(&[finalized], &[file], &graph)
            .unwrap();
        env_loader.get_cached(&project_dir)
    }

    fn var(
        env: &Option<std::sync::Arc<crate::EnvVarsMap>>,
        key: &str,
    ) -> Option<String> {
        env.as_ref().and_then(|e| e.get(key).cloned())
    }

    #[test]
    fn workspace_vars_reach_a_project_without_env_vars() {
        let env = resolved_env(r#"{"X":"w"}"#, None, false, None, false);
        assert_eq!(var(&env, "X").as_deref(), Some("w"));
    }

    #[test]
    fn workspace_vars_reach_a_project_at_the_workspace_root() {
        let env = resolved_env(r#"{"X":"w"}"#, None, true, None, false);
        assert_eq!(var(&env, "X").as_deref(), Some("w"));
    }

    #[test]
    fn workspace_and_project_vars_both_reach_the_project() {
        let env = resolved_env(
            r#"{"X":"w"}"#,
            Some(r#"{"Y":"p"}"#),
            false,
            None,
            false,
        );
        assert_eq!(var(&env, "X").as_deref(), Some("w"));
        assert_eq!(var(&env, "Y").as_deref(), Some("p"));
    }

    #[test]
    fn a_workspace_value_wins_over_the_project_value() {
        let env = resolved_env(
            r#"{"K":"w"}"#,
            Some(r#"{"K":"p"}"#),
            false,
            None,
            false,
        );
        assert_eq!(var(&env, "K").as_deref(), Some("w"));
    }

    #[test]
    fn project_vars_expand_from_the_workspace_vars() {
        let env = resolved_env(
            r#"{"X":"w"}"#,
            Some(r#"{"Y":"${X}-p"}"#),
            false,
            None,
            false,
        );
        assert_eq!(var(&env, "Y").as_deref(), Some("w-p"));
        let env = resolved_env(
            r#"{"X":"w"}"#,
            Some(r#"{"X":"p","Y":"${X}"}"#),
            false,
            None,
            false,
        );
        assert_eq!(var(&env, "X").as_deref(), Some("w"));
        assert_eq!(var(&env, "Y").as_deref(), Some("p"));
    }

    #[test]
    fn a_workspace_var_wins_over_the_dotenv_value_without_project_vars() {
        let env =
            resolved_env(r#"{"X":"w"}"#, None, false, Some("X=file\n"), false);
        assert_eq!(var(&env, "X").as_deref(), Some("w"));
    }

    #[test]
    fn no_workspace_vars_adds_nothing_and_definitions_only_loads_nothing() {
        let env = resolved_env("{}", None, false, None, false);
        assert_eq!(var(&env, "X"), None);
        let env = resolved_env(r#"{"X":"w"}"#, None, false, None, true);
        assert_eq!(var(&env, "X"), None);
    }

    /// RFC 0024 F03.4 slice 0: projects `a#b`/`c` and `a`/`b#c` rendered the
    /// same joined key, so one task's data replaced the other's.
    #[tokio::test]
    async fn formerly_colliding_pairs_keep_their_own_extracted_data() {
        use omni_configurations::ProjectLayer;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mut layers = vec![];
        let mut files = vec![];
        for (sub, name, task, who) in
            [("ab", "a#b", "c", "ab-c"), ("a", "a", "b\\#c", "a-bc")]
        {
            std::fs::create_dir_all(root.join(sub)).unwrap();
            let file = root.join(sub).join("project.omni.yaml");
            let json = serde_json::json!({
                "name": name,
                "tasks": {task: {
                    "exec": "x",
                    "env": {"vars": {"WHO": who}},
                    "meta": {"who": who},
                    "cache": {"enabled": who == "ab-c"},
                }},
            });
            layers.push(layer(file.to_str().unwrap(), &[]).with_layer(
                serde_json::from_value::<ProjectLayer>(json).unwrap(),
            ));
            files.push(file);
        }
        let graph = ExtensionGraph::from_nodes(layers.clone()).unwrap();
        let finalized = layers
            .into_iter()
            .map(|l| l.finalize_composed().unwrap())
            .collect::<Vec<_>>();
        let workspace: WorkspaceConfiguration =
            serde_json::from_str(r#"{"projects":["**"]}"#).unwrap();
        let mut env_loader = EnvLoader::new(RealSys, root.clone(), vec![]);
        let data = ProjectDataExtractor::new(
            &RealSys,
            &root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&finalized, &files, &graph)
        .unwrap();

        let env = |p: &str, t: &str| {
            data.task_env_var_overrides.get(p, t).unwrap()["WHO"].clone()
        };
        assert_eq!(env("a#b", "c"), "ab-c");
        assert_eq!(env("a", "b#c"), "a-bc");
        assert!(data.cache_infos.get("a#b", "c").unwrap().cache_enabled);
        assert!(!data.cache_infos.get("a", "b#c").unwrap().cache_enabled);
        assert!(data.cache_infos.get("a", "c").is_none());
        assert_eq!(data.cache_infos.len(), 2);
        assert_eq!(data.task_meta_configs.len(), 2);
        assert_eq!(data.output_logs_configs.len(), 0);
        assert_eq!(data.redact_policies.len(), 2);
        let who = |p: &str, t: &str| {
            data.task_meta_configs.get(p, t).unwrap().0.as_map()["who"]
                .clone()
                .into_json()
        };
        assert_eq!(who("a#b", "c"), serde_json::json!("ab-c"));
        assert_eq!(who("a", "b#c"), serde_json::json!("a-bc"));
    }

    /// RFC 0024 F03.4 slice 2: aliases travel with the project, never become
    /// tasks or per-task data, and a root reference may name one.
    #[test]
    fn aliases_reach_the_project_and_are_not_tasks() {
        use omni_configurations::ProjectLayer;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let file = root.join("app/project.omni.yaml");
        let l = layer(file.to_str().unwrap(), &[]).with_layer(
            serde_json::from_value::<ProjectLayer>(serde_json::json!({
                "name": "app",
                "tasks": {
                    "build-native": "make",
                    "compile": { "alias": "build-native" },
                    "t": { "dependencies": ["compile"] }
                }
            }))
            .unwrap(),
        );
        let graph = ExtensionGraph::from_nodes(vec![l.clone()]).unwrap();
        let finalized = l.finalize_composed().unwrap();
        let workspace: WorkspaceConfiguration =
            serde_json::from_str(r#"{"projects":["**"]}"#).unwrap();
        let mut env_loader = EnvLoader::new(RealSys, root.clone(), vec![]);
        let data = ProjectDataExtractor::new(
            &RealSys,
            &root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&[finalized], &[file], &graph)
        .unwrap();
        let project = &data.projects[0];
        assert_eq!(project.aliases.resolve("compile"), Some("build-native"));
        assert!(!project.tasks.contains_key("compile"));
        assert!(data.cache_infos.get("app", "compile").is_none());
        assert_eq!(
            project.canonical_task_name("compile"),
            Some("build-native")
        );
    }

    /// F07.2 parity gate: cache-key file inputs `extract_all` derives from the
    /// extension graph, captured from the pre-F07 revision (c7aa81fc). The
    /// baseline's list ORDER varies from run to run (hash-ordered), so the
    /// sorted multiset is pinned, repeated entries included (the project
    /// file appears twice), plus the stable position of the project's own
    /// file at the end. Paths are relative to the temp root and joined
    /// with `/`, since the absolute temp root is not stable and the cache
    /// extractor exposes inputs, not digests.
    fn extracted_inputs(
        files: &[(&str, &str)],
        app: &str,
    ) -> Vec<(String, Vec<String>, Vec<String>, bool)> {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let root_json = root.to_string_lossy().replace('\\', "\\\\");
        let mut layers = vec![];
        for (name, json) in files {
            let file = root.join(name);
            let dir = file.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&file, "").unwrap();
            layers.push(LoadedLayer::from_file(
                file.into(),
                dir.into(),
                serde_json::from_str(&json.replace("ROOT", &root_json))
                    .unwrap(),
            ));
        }
        let graph = ExtensionGraph::from_nodes(layers.clone()).unwrap();
        let app_layer = layers
            .into_iter()
            .find(|l| l.file().path().unwrap() == root.join(app))
            .unwrap();
        let finalized = app_layer.finalize_composed().unwrap();
        let workspace = serde_json::from_str(r#"{"projects":["**"]}"#).unwrap();
        let mut env_loader = EnvLoader::new(RealSys, root.to_owned(), vec![]);
        let data = ProjectDataExtractor::new(
            &RealSys,
            root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&[finalized], &[root.join(app)], &graph)
        .unwrap();
        let rel = |p: &OmniPath| {
            p.path()
                .unwrap()
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        };
        ["t", "n"]
            .into_iter()
            .map(|task| {
                let info = data.cache_infos.get("app", task).unwrap();
                let mut include = info
                    .key_input_files
                    .include
                    .iter()
                    .map(|g| rel(g))
                    .collect::<Vec<_>>();
                // The project's own file is pushed last, after the closure
                // (whose order is hash-random), so its position is stable
                // and worth pinning before the list is sorted.
                if info.key_defaults {
                    assert_eq!(
                        include.last().map(String::as_str),
                        Some("app/project.omni.yaml"),
                        "the project's own file must be the last input",
                    );
                }
                include.sort();
                let mut exclude = info
                    .key_input_files
                    .exclude
                    .iter()
                    .map(|g| rel(g))
                    .collect::<Vec<_>>();
                exclude.sort();
                (task.to_string(), include, exclude, info.key_defaults)
            })
            .collect()
    }

    const APP_TASKS: &str = r#""tasks":{
        "t":{"exec":"x"},
        "n":{"exec":"x","cache":{"key":{"defaults":false}}}}"#;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_file_backed_extension_chain_keeps_its_cache_inputs() {
        let app = format!(
            r#"{{"name":"app","extends":["ROOT/lib/mid.yaml"],{APP_TASKS}}}"#
        );
        let got = extracted_inputs(
            &[
                ("lib/base.yaml", r#"{"base":true}"#),
                (
                    "lib/mid.yaml",
                    r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#,
                ),
                ("app/project.omni.yaml", &app),
            ],
            "app/project.omni.yaml",
        );
        assert_eq!(
            got,
            vec![
                (
                    "t".to_string(),
                    strings(&[
                        "app/project.omni.yaml",
                        "app/project.omni.yaml",
                        "lib/base.yaml",
                        "lib/mid.yaml",
                    ]),
                    vec![],
                    true
                ),
                ("n".to_string(), vec![], vec![], false),
            ]
        );
    }

    #[test]
    fn a_diamond_extension_keeps_its_cache_inputs() {
        let app = format!(
            r#"{{"name":"app","extends":["ROOT/lib/left.yaml","ROOT/lib/right.yaml"],{APP_TASKS}}}"#
        );
        let got = extracted_inputs(
            &[
                ("lib/base.yaml", r#"{"base":true}"#),
                (
                    "lib/left.yaml",
                    r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#,
                ),
                (
                    "lib/right.yaml",
                    r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#,
                ),
                ("app/project.omni.yaml", &app),
            ],
            "app/project.omni.yaml",
        );
        assert_eq!(
            got,
            vec![
                (
                    "t".to_string(),
                    strings(&[
                        "app/project.omni.yaml",
                        "app/project.omni.yaml",
                        "lib/base.yaml",
                        "lib/left.yaml",
                        "lib/right.yaml",
                    ]),
                    vec![],
                    true
                ),
                ("n".to_string(), vec![], vec![], false),
            ]
        );
    }

    /// F07.2 digest parity gate. Feeds `extract_all`'s cache inputs for the
    /// `t` and `n` tasks of `app` through the collector's digest step, the
    /// same call `ProjectHasher::hash` makes per task (it then only combines
    /// the per-task digests). The files are empty and the layout fixed, so
    /// the digest depends only on the relative input paths, which the hasher
    /// renders with the native separator. `closure_order` permutes the
    /// extracted `include` list, standing in for the hash-random closure
    /// order of `ExtensionGraph::get_transitive_extendee_ids`.
    async fn task_digests(
        files: &[(&str, &str)],
        app: &str,
        closure_order: fn(&mut Vec<OmniPath>),
    ) -> Vec<(String, String)> {
        task_digests_in(files, app, r#"{"projects":["**"]}"#, closure_order)
            .await
    }

    async fn task_digests_in(
        files: &[(&str, &str)],
        app: &str,
        workspace_json: &str,
        closure_order: fn(&mut Vec<OmniPath>),
    ) -> Vec<(String, String)> {
        use omni_collector::{CollectConfig, Collector, ProjectTaskInfo};

        let temp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let root = temp.path();
        let root_json = root.to_string_lossy().replace('\\', "\\\\");
        let mut layers = vec![];
        for (name, json) in files {
            let file = root.join(name);
            let dir = file.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&file, "").unwrap();
            layers.push(LoadedLayer::from_file(
                file.into(),
                dir.into(),
                serde_json::from_str(&json.replace("ROOT", &root_json))
                    .unwrap(),
            ));
        }
        let mut graph = ExtensionGraph::from_nodes(layers).unwrap();
        // Compose the extension closure the way the loader does, so inherited
        // settings reach the finalized project.
        let app_layer = graph
            .get_or_process_all_nodes()
            .unwrap()
            .into_iter()
            .find(|l| l.file().path().unwrap() == root.join(app))
            .unwrap();
        let finalized = app_layer.finalize_composed().unwrap();
        let workspace = serde_json::from_str(workspace_json).unwrap();
        let mut env_loader = EnvLoader::new(RealSys, root.to_owned(), vec![]);
        let data = ProjectDataExtractor::new(
            &RealSys,
            root,
            &mut env_loader,
            false,
            &workspace,
        )
        .extract_all(&[finalized], &[root.join(app)], &graph)
        .unwrap();
        let project = data.projects.iter().find(|p| p.name == "app").unwrap();
        let no_env = Map::default();
        // The per-task call `ProjectHasher::hash` makes: the plan node, the
        // unexpanded cache info and the meta fallback feed the one builder.
        let task_graph =
            omni_core::ProjectGraph::from_projects(data.projects.clone())
                .unwrap()
                .get_task_execution_graph()
                .unwrap();
        let mut out = vec![];
        for task in ["t", "n"] {
            let info = data.cache_infos.get("app", task).unwrap();
            let node = task_graph.get_task_by_name("app", task).unwrap();
            let effective_digest = omni_task_context::effective_task::build(
                node,
                Some(info),
                data.task_meta_configs
                    .get("app", task)
                    .or_else(|| data.project_meta_configs.get("app")),
            )
            .digest();
            let mut include = info.key_input_files.include.clone();
            closure_order(&mut include);
            let task_info = ProjectTaskInfo {
                project_name: "app",
                project_dir: &project.dir,
                task_name: task,
                task_exec: node.task_exec_config(),
                task_retry_exec: node.task_retry_exec_config(),
                output_files: &info.cache_output_files.include,
                output_files_exclude: &info.cache_output_files.exclude,
                input_files: &include,
                input_files_exclude: &info.key_input_files.exclude,
                input_env_keys: &info.key_env_keys,
                env_vars: &no_env,
                dependency_digests: &[],
                args: &info.args,
                effective_digest,
            };
            let collected = Collector::new(root, cache.path(), RealSys)
                .collect(
                    &[task_info],
                    &CollectConfig {
                        input_files: true,
                        output_files: false,
                        digests: true,
                        cache_output_dirs: false,
                    },
                )
                .await
                .unwrap();
            let digest = collected[0].digest.expect("digest");
            out.push((
                task.to_string(),
                bs58::encode(digest.as_ref()).into_string(),
            ));
        }
        out
    }

    fn chain_files(app: &str) -> Vec<(&'static str, String)> {
        vec![
            ("lib/base.yaml", r#"{"base":true}"#.to_string()),
            (
                "lib/mid.yaml",
                r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#.to_string(),
            ),
            ("app/project.omni.yaml", app.to_string()),
        ]
    }

    fn diamond_files(app: &str) -> Vec<(&'static str, String)> {
        vec![
            ("lib/base.yaml", r#"{"base":true}"#.to_string()),
            (
                "lib/left.yaml",
                r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#.to_string(),
            ),
            (
                "lib/right.yaml",
                r#"{"base":true,"extends":["ROOT/lib/base.yaml"]}"#.to_string(),
            ),
            ("app/project.omni.yaml", app.to_string()),
        ]
    }

    async fn digests_for(
        files: Vec<(&'static str, String)>,
        order: fn(&mut Vec<OmniPath>),
    ) -> Vec<(String, String)> {
        let refs = files
            .iter()
            .map(|(n, j)| (*n, j.as_str()))
            .collect::<Vec<_>>();
        task_digests(&refs, "app/project.omni.yaml", order).await
    }

    fn keep(_: &mut Vec<OmniPath>) {}
    fn reverse(v: &mut Vec<OmniPath>) {
        v.reverse();
    }
    fn rotate(v: &mut Vec<OmniPath>) {
        if !v.is_empty() {
            v.rotate_left(1);
        }
    }

    #[tokio::test]
    async fn chain_task_digests_match_the_baseline_and_ignore_closure_order() {
        let app = format!(
            r#"{{"name":"app","extends":["ROOT/lib/mid.yaml"],{APP_TASKS}}}"#
        );
        let a = digests_for(chain_files(&app), keep).await;
        let b = digests_for(chain_files(&app), reverse).await;
        let c = digests_for(chain_files(&app), rotate).await;
        assert_eq!(a, b);
        assert_eq!(a, c);
        // Separators are hashed natively, so the literals are Unix-only.
        #[cfg(unix)]
        assert_eq!(
            a,
            CHAIN_DIGESTS.map(|(t, d)| (t.to_string(), d.to_string()))
        );
    }

    #[tokio::test]
    async fn diamond_task_digests_match_the_baseline_and_ignore_closure_order()
    {
        let app = format!(
            r#"{{"name":"app","extends":["ROOT/lib/left.yaml","ROOT/lib/right.yaml"],{APP_TASKS}}}"#
        );
        let a = digests_for(diamond_files(&app), keep).await;
        let b = digests_for(diamond_files(&app), reverse).await;
        let c = digests_for(diamond_files(&app), rotate).await;
        assert_eq!(a, b);
        assert_eq!(a, c);
        #[cfg(unix)]
        assert_eq!(
            a,
            DIAMOND_DIGESTS.map(|(t, d)| (t.to_string(), d.to_string()))
        );
    }

    // Re-pinned for the cache key version and effective-task leaves (RFC 0024
    // F06.3). Before: chain t EkZdvw62..., n BtEuC55R...; diamond t x7qpHaig...,
    // n BtEuC55R.... The old values reproduce with the two leaves switched off.
    // Re-pinned again when the env definitions became three unmerged layers
    // (fix round 2, new encoding bytes). Before: chain t 2ZN4BeGR..., n
    // m2Yfawkt...; diamond t GgerQQzk..., n m2Yfawkt.... They reproduce with the
    // layers merged into the single env field.
    #[cfg(unix)]
    const CHAIN_DIGESTS: [(&str, &str); 2] = [
        ("t", "Gf1zXEUfnGTFGvE9M1K6fXkYEuBWXXvwRPW1YwGnE749"),
        ("n", "9noLmyciPdkxSAuNkhiWx8KHx8fVbLTH5r2GePusza9F"),
    ];
    #[cfg(unix)]
    const DIAMOND_DIGESTS: [(&str, &str); 2] = [
        ("t", "CbyWCFVZP7UEBtDba6xem58oDzgswenaBmDEqBXd9juo"),
        ("n", "9noLmyciPdkxSAuNkhiWx8KHx8fVbLTH5r2GePusza9F"),
    ];

    // RFC 0024 F06.3 proof. The final key must follow the *effective* task:
    // what composition and inheritance leave after the raw text is gone. Both
    // tasks turn the default config-file inputs off, so no file content can
    // stand in for the definition: only an effective-task contribution can.
    const NO_DEFAULTS: &str = r#""cache":{"key":{"defaults":false}}"#;

    fn effective_app(app_meta: &str, t_description: &str) -> String {
        format!(
            r#"{{"name":"app","extends":["ROOT/lib/base.yaml"]{app_meta},"tasks":{{
                "t":{{"exec":"echo {{{{ meta.flavor }}}}",{NO_DEFAULTS}{t_description}}},
                "n":{{"exec":"x",{NO_DEFAULTS}}}}}}}"#
        )
    }

    fn effective_files(
        base_flavor: &str,
        app: &str,
    ) -> Vec<(&'static str, String)> {
        vec![
            (
                "lib/base.yaml",
                format!(
                    r#"{{"base":true,"meta":{{"flavor":"{base_flavor}"}}}}"#
                ),
            ),
            ("app/project.omni.yaml", app.to_string()),
        ]
    }

    async fn effective_digests(
        base_flavor: &str,
        app: &str,
    ) -> Vec<(String, String)> {
        digests_for(effective_files(base_flavor, app), keep).await
    }

    fn digest_of<'a>(all: &'a [(String, String)], task: &str) -> &'a str {
        all.iter()
            .find(|(t, _)| t == task)
            .map(|(_, d)| d.as_str())
            .unwrap()
    }

    /// FAILS on the base: the collector hashes the raw command text, which is
    /// the same, and nothing else sees the inherited metadata it consumes.
    #[tokio::test]
    async fn an_inherited_setting_change_changes_the_final_key() {
        let app = effective_app("", "");
        let one = effective_digests("one", &app).await;
        let two = effective_digests("two", &app).await;
        assert_ne!(
            digest_of(&one, "t"),
            digest_of(&two, "t"),
            "inherited metadata consumed by the command must reach the key"
        );
    }

    /// FAILS on the base. The unchanged-command guard of the same defect: a
    /// definition change must invalidate even with `cache.key.defaults: false`.
    #[tokio::test]
    async fn defaults_off_still_invalidates_on_a_definition_change() {
        let one = effective_digests("one", &effective_app("", "")).await;
        let other = effective_digests(
            "one",
            &effective_app(r#","meta":{"flavor":"local"}"#, ""),
        )
        .await;
        assert_ne!(digest_of(&one, "t"), digest_of(&other, "t"));
    }

    /// Guard, passes on the base and must keep passing: a change to one task's
    /// own definition does not move an unrelated sibling's key.
    #[tokio::test]
    async fn an_unrelated_sibling_change_keeps_this_tasks_key() {
        let before = effective_digests("one", &effective_app("", "")).await;
        let changed =
            effective_app("", "").replace(r#""exec":"x""#, r#""exec":"y""#);
        let after = effective_digests("one", &changed).await;
        assert_eq!(digest_of(&before, "t"), digest_of(&after, "t"));
        assert_ne!(digest_of(&before, "n"), digest_of(&after, "n"));
    }

    /// Guard: description is presentation, not execution.
    #[tokio::test]
    async fn a_presentation_only_change_keeps_the_key() {
        let before = effective_digests("one", &effective_app("", "")).await;
        let after = effective_digests(
            "one",
            &effective_app("", r#","description":"a new description""#),
        )
        .await;
        assert_eq!(before, after);
    }

    /// Guard: the same effective semantics reached two ways share a key.
    #[tokio::test]
    async fn identical_effective_semantics_keep_the_key() {
        let inherited = effective_digests("one", &effective_app("", "")).await;
        let restated = effective_digests(
            "zzz",
            &effective_app(r#","meta":{"flavor":"one"}"#, ""),
        )
        .await;
        assert_eq!(digest_of(&inherited, "t"), digest_of(&restated, "t"));
    }

    /// Guard: an explicit override masks an inherited change that no longer
    /// reaches the effective task.
    #[tokio::test]
    async fn an_explicit_override_masks_an_irrelevant_inherited_change() {
        let app = effective_app(r#","meta":{"flavor":"mine"}"#, "");
        let one = effective_digests("one", &app).await;
        let two = effective_digests("two", &app).await;
        assert_eq!(digest_of(&one, "t"), digest_of(&two, "t"));
    }

    fn env_files(base_env: &str, app: &str) -> Vec<(&'static str, String)> {
        vec![
            (
                "lib/base.yaml",
                format!(
                    r#"{{"base":true,"env":{{"vars":{{"X":"{base_env}"}}}}}}"#
                ),
            ),
            ("app/project.omni.yaml", app.to_string()),
        ]
    }

    /// FAILS on the base (Q3): an `env.vars` definition inherited from a base
    /// file is part of the effective task. The key records only the values of
    /// the keys listed under `cache.key.env`, so the change is invisible.
    #[tokio::test]
    async fn an_inherited_project_env_definition_change_changes_the_final_key()
    {
        let app = effective_app("", "");
        let one = digests_for(env_files("one", &app), keep).await;
        let two = digests_for(env_files("two", &app), keep).await;
        assert_ne!(
            digest_of(&one, "t"),
            digest_of(&two, "t"),
            "inherited project env.vars must reach the key"
        );
    }

    /// FAILS on the base (Q3): the workspace `env.vars` definition.
    #[tokio::test]
    async fn a_workspace_env_definition_change_changes_the_final_key() {
        let app = effective_app("", "");
        let files = effective_files("one", &app);
        let refs = files
            .iter()
            .map(|(n, j)| (*n, j.as_str()))
            .collect::<Vec<_>>();
        let digests = |value: &str| {
            let workspace = format!(
                r#"{{"projects":["**"],"env":{{"vars":{{"X":"{value}"}}}}}}"#
            );
            let refs = refs.clone();
            async move {
                task_digests_in(
                    &refs,
                    "app/project.omni.yaml",
                    &workspace,
                    keep,
                )
                .await
            }
        };
        let one = digests("one").await;
        let two = digests("two").await;
        assert_ne!(
            digest_of(&one, "t"),
            digest_of(&two, "t"),
            "workspace env.vars must reach the key"
        );
    }

    /// The `t` digest for a workspace, a project and a task that each may
    /// define `X`. `None` leaves that layer without any `env.vars`.
    async fn layered_env_digest(
        workspace: Option<&str>,
        project: Option<&str>,
        task: Option<&str>,
    ) -> String {
        let project_env = project
            .map(|v| format!(r#","env":{{"vars":{{"X":"{v}"}}}}"#))
            .unwrap_or_default();
        let task_env = task
            .map(|v| format!(r#""env":{{"vars":{{"X":"{v}"}}}},"#))
            .unwrap_or_default();
        let app = effective_app(&project_env, "")
            .replace(r#""t":{"#, &format!(r#""t":{{{task_env}"#));
        let workspace = match workspace {
            Some(v) => format!(
                r#"{{"projects":["**"],"env":{{"vars":{{"X":"{v}"}}}}}}"#
            ),
            None => r#"{"projects":["**"]}"#.to_string(),
        };
        let files = effective_files("one", &app);
        let refs = files
            .iter()
            .map(|(n, j)| (*n, j.as_str()))
            .collect::<Vec<_>>();
        let all =
            task_digests_in(&refs, "app/project.omni.yaml", &workspace, keep)
                .await;
        digest_of(&all, "t").to_string()
    }

    /// FAILS before the fix. At runtime the workspace value wins over the
    /// project value of a key both define, so changing only the workspace
    /// value changes the task environment and must change the key.
    #[tokio::test]
    async fn a_colliding_workspace_value_change_changes_the_final_key() {
        let one = layered_env_digest(Some("w1"), Some("p"), None).await;
        let two = layered_env_digest(Some("w2"), Some("p"), None).await;
        assert_ne!(one, two, "the winning workspace value must reach the key");
    }

    /// FAILS before the fix. Expansion starts from the project's raw
    /// definitions and only afterwards do the expanded workspace values
    /// overwrite colliding keys, so a project variable reading a key the
    /// workspace also defines (`Y: ${X}`) sees the PROJECT's `X`. Changing the
    /// shadowed project value therefore changes the task's `Y` and must change
    /// the key. The layers are keyed unmerged: this over-invalidates when the
    /// shadowed value is not read, which is the safe direction.
    #[tokio::test]
    async fn a_shadowed_project_value_change_changes_the_final_key() {
        let one = layered_env_digest(Some("w"), Some("p1"), None).await;
        let two = layered_env_digest(Some("w"), Some("p2"), None).await;
        assert_ne!(one, two, "the layers are keyed unmerged");
    }

    /// FAILS before the fix: the same case with the reading variable present,
    /// as at runtime (`X` and `Y: ${X}` in the project, `X` in the workspace).
    #[tokio::test]
    async fn a_project_variable_reading_a_workspace_key_follows_the_project_value()
     {
        let key = |p: &'static str| async move {
            let app = effective_app(
                &format!(r#","env":{{"vars":{{"X":"{p}","Y":"${{X}}"}}}}"#),
                "",
            );
            let ws =
                r#"{"projects":["**"],"env":{"vars":{"X":"w"}}}"#.to_string();
            let files = effective_files("one", &app);
            let refs = files
                .iter()
                .map(|(n, j)| (*n, j.as_str()))
                .collect::<Vec<_>>();
            let all =
                task_digests_in(&refs, "app/project.omni.yaml", &ws, keep)
                    .await;
            digest_of(&all, "t").to_string()
        };
        assert_ne!(key("p1").await, key("p2").await);
    }

    /// Guards: a key only one layer defines, and the task layer.
    #[tokio::test]
    async fn non_colliding_and_task_layers_keep_following_the_key() {
        let project_only = |p| layered_env_digest(None, Some(p), None);
        assert_ne!(project_only("p1").await, project_only("p2").await);
        let workspace_only = |w| layered_env_digest(Some(w), None, None);
        assert_ne!(workspace_only("w1").await, workspace_only("w2").await);
        let task = |w, p, t| layered_env_digest(Some(w), Some(p), Some(t));
        assert_ne!(task("w", "p", "t1").await, task("w", "p", "t2").await);
        // Unmerged: a task value does not hide the lower layers either, since
        // an expansion in the task can still read them.
        assert_ne!(task("w1", "p", "t").await, task("w2", "p", "t").await);
        assert_ne!(task("w", "p1", "t").await, task("w", "p2", "t").await);
    }
}
