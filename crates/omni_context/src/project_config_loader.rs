use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use derive_new::new;
use enum_map::enum_map;
use omni_configurations::{
    FinalizeError, FinalizedProject, LoadConfigError, LoadConfigErrorKind,
    LoadedLayer, ProjectLayer, TaskExtensionError, types::SingleOrMany,
};
use omni_core::{ExtensionGraph, ExtensionGraphError};
use omni_types::{OmniPath, Root};
use path_clean::clean;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use thiserror::Error;
use tracing::Level;

use crate::ContextSys;

#[derive(Debug, Clone, Copy, new)]
pub struct ProjectConfigLoader<'a, TSys: ContextSys> {
    #[new(into)]
    sys: &'a TSys,
    #[new(into)]
    root_dir: &'a Path,
}

#[derive(new, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
struct RelatedPath {
    path: PathBuf,
    extender: Option<PathBuf>,
}

impl<'a, TSys: ContextSys> ProjectConfigLoader<'a, TSys> {
    #[tracing::instrument(level = Level::DEBUG, skip(self))]
    #[allow(clippy::result_large_err)]
    pub async fn load_project_layers(
        &self,
        project_paths: &[PathBuf],
    ) -> Result<Vec<LoadedLayer>, ProjectConfigLoaderError> {
        let mut layers = vec![];
        let mut loaded = HashSet::new();
        let mut to_process = project_paths
            .iter()
            .map(|p| RelatedPath::new(p.clone(), None))
            .collect::<Vec<_>>();
        to_process.sort();
        let root_dir = &self.root_dir;

        while let Some(conf) = to_process.pop() {
            // A discovered config can also be queued as an extendee before its
            // own discovered entry is popped, so skip anything already loaded.
            let file: OmniPath = conf.path.clone().into();
            if loaded.contains(&file) {
                continue;
            }

            let start_time = std::time::SystemTime::now();

            let mut layer = ProjectLayer::load(&conf.path as &Path, self.sys)
                .await
                .map_err(|e| {
                    if e.kind() == LoadConfigErrorKind::FileNotFound {
                        if let Some(extendee_path) = &conf.extender {
                            ProjectConfigLoaderErrorInner::ExtendedProjectConfigDoesNotExist {
                                extender_path: extendee_path.clone(),
                                extendee_path: conf.path.clone(),
                            }
                        } else {
                            ProjectConfigLoaderErrorInner::ProjectConfigDoesNotExist {
                                path: conf.path.clone(),
                            }
                        }
                    } else {
                        ProjectConfigLoaderErrorInner::LoadConfig(e)
                    }
                })?;

            let elapsed = start_time.elapsed().unwrap_or_default();
            tracing::trace!(
                project_layer = ?layer,
                "loaded project configuration file {:?} in {} ms",
                conf,
                elapsed.as_millis()
            );

            let project_dir = conf.path.parent().ok_or_else(|| {
                ProjectConfigLoaderErrorInner::NoParentDirFoundForPath(
                    conf.path.clone(),
                )
            })?;
            loaded.insert(file.clone());

            let bases = enum_map! {
                Root::Workspace => root_dir,
                Root::Project => project_dir,
            };

            // Resolve @project paths against this layer's own directory.
            layer.resolve_origin_paths(&bases);

            for extended_parent in
                layer.extends.iter_mut().flat_map(SingleOrMany::iter_mut)
            {
                if extended_parent.is_any_rooted() {
                    extended_parent.resolve_in_place(&bases);
                }

                let extended_path = clean(project_dir.join(
                    extended_parent.path().expect("path should be resolved"),
                ));

                if !extended_path.starts_with(root_dir) {
                    return Err(ProjectConfigLoaderErrorInner::ExtendedProjectConfigOutsideWorkspace {
                        extender_path: conf.path.clone(),
                        extendee_path: extended_path,
                    }
                    .into());
                }

                *extended_parent = extended_path.into();

                if !loaded.contains(extended_parent) {
                    to_process.push(RelatedPath::new(
                        extended_parent
                            .path()
                            .expect("path should be resolved")
                            .to_path_buf(),
                        Some(conf.path.clone()),
                    ));
                }
            }

            layers.push(LoadedLayer {
                file,
                dir: project_dir.into(),
                layer,
            });
        }

        Ok(layers)
    }

    /// Load every layer, compose each project's extension closure by
    /// replaying the raw layers in extension order, then finalize: task
    /// extensions and project-to-task inheritance first, defaults and
    /// validation last.
    #[tracing::instrument(level = Level::DEBUG, skip(self))]
    #[allow(clippy::result_large_err)]
    pub async fn load_project_configs(
        &self,
        project_paths: &[PathBuf],
    ) -> Result<LoadedProjects, ProjectConfigLoaderError> {
        let layers = self.load_project_layers(project_paths).await?;
        let mut extension_graph = ExtensionGraph::from_nodes(layers)?;
        let projects = extension_graph
            .get_or_process_all_nodes()?
            .into_iter()
            .map(|composed| {
                let path = composed
                    .file
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                composed.finalize_composed().map_err(|e| match e {
                    FinalizeError::MissingName(_) => {
                        ProjectConfigLoaderErrorInner::MissingProjectName {
                            path,
                        }
                    }
                    FinalizeError::TaskExtension(e) => e.into(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LoadedProjects {
            projects,
            extension_graph,
        })
    }
}

/// Finalized projects and the extension graph they were composed from.
#[derive(Debug)]
pub struct LoadedProjects {
    pub projects: Vec<FinalizedProject>,
    pub extension_graph: ExtensionGraph<LoadedLayer>,
}

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct ProjectConfigLoaderError(pub(crate) ProjectConfigLoaderErrorInner);

impl ProjectConfigLoaderError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectConfigLoaderErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectConfigLoaderErrorInner>> From<T>
    for ProjectConfigLoaderError
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Error, Debug, EnumDiscriminants, new, omni_diagnostic::Diagnostic)]
#[strum_discriminants(vis(pub), name(ProjectConfigLoaderErrorKind))]
pub(crate) enum ProjectConfigLoaderErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    LoadConfig(#[from] LoadConfigError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExtensionGraph(#[from] ExtensionGraphError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    TaskExtension(#[from] TaskExtensionError),

    #[error("no parent dir found for path: {0}")]
    #[diagnostic(skip)]
    NoParentDirFoundForPath(PathBuf),

    /// A project extends a configuration file that does not exist.
    #[error("project configuration file does not exist: {extendee}, loaded by {extender}", extender = extender_path.display(), extendee = extendee_path.display())]
    #[diagnostic(
        code = "workspace.extends_not_found",
        hint = "fix the `extends` entry in {extender_path:?} so it points at an existing project configuration"
    )]
    ExtendedProjectConfigDoesNotExist {
        #[diagnostic(detail = "extender")]
        extender_path: PathBuf,
        #[diagnostic(detail = "extendee")]
        extendee_path: PathBuf,
    },

    /// A project extends a configuration file outside the workspace root.
    #[error("project configuration file is outside the workspace root: {extendee}, extended by {extender}", extender = extender_path.display(), extendee = extendee_path.display())]
    #[diagnostic(
        code = "workspace.extends_outside_workspace",
        hint = "`extends` in {extender_path:?} may only reference files inside the workspace root; move or copy the file into the workspace"
    )]
    ExtendedProjectConfigOutsideWorkspace {
        #[diagnostic(detail = "extender")]
        extender_path: PathBuf,
        #[diagnostic(detail = "extendee")]
        extendee_path: PathBuf,
    },

    /// A project has no `name` once its layers are composed.
    #[error("project configuration has no `name`: {path}", path = path.display())]
    #[diagnostic(
        code = "workspace.project_name_missing",
        hint = "add a `name` to {path:?} or to a file it extends"
    )]
    MissingProjectName {
        #[diagnostic(detail)]
        path: PathBuf,
    },

    /// A discovered project configuration file does not exist.
    #[error("project configuration file does not exist: {path}", path = path.display())]
    #[diagnostic(
        code = "workspace.project_config_not_found",
        hint = "check that {path:?} exists and that the workspace `projects` globs match it"
    )]
    ProjectConfigDoesNotExist {
        #[diagnostic(detail)]
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use system_traits::impls::RealSys;

    use super::*;
    use crate::test_fixture::tmp;

    fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    async fn layers(root: &Path, files: &[PathBuf]) -> Vec<LoadedLayer> {
        ProjectConfigLoader::new(&RealSys, root)
            .load_project_layers(files)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn layer_keeps_absent_fields_and_its_own_origin() {
        let dir = tmp();
        let root = dir.path();
        let file = write(root, "app/project.omni.yaml", "name: app\n");

        let layers = layers(root, std::slice::from_ref(&file)).await;

        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].file, OmniPath::from(file));
        assert_eq!(layers[0].dir, OmniPath::from(root.join("app")));
        assert_eq!(layers[0].layer.name.as_deref(), Some("app"));
        assert!(layers[0].layer.cache.is_none());
        assert!(layers[0].layer.tasks.is_none());
    }

    #[tokio::test]
    async fn a_layer_without_a_name_loads() {
        let dir = tmp();
        let root = dir.path();
        let file = write(root, "app/project.omni.yaml", "base: true\n");

        let layers = layers(root, &[file]).await;

        assert_eq!(layers[0].layer.name, None);
    }

    #[tokio::test]
    async fn project_paths_resolve_against_each_layers_own_directory() {
        let dir = tmp();
        let root = dir.path();
        let base = write(
            root,
            "shared/base.omni.yaml",
            "name: base\ncache:\n  key:\n    files: ['@project/in.txt']\ntasks:\n  t:\n    cache:\n      output:\n        files: ['@project/out']\n",
        );
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../shared/base.omni.yaml\n",
        );

        let layers = layers(root, &[app]).await;

        let by_file = |f: &Path| {
            layers.iter().find(|l| l.file == OmniPath::from(f)).unwrap()
        };
        let base = by_file(&base);
        let key_files = base
            .layer
            .cache
            .clone()
            .unwrap()
            .key
            .unwrap()
            .files
            .unwrap()
            .normalize();
        assert_eq!(
            key_files.include[0].path().unwrap(),
            root.join("shared/in.txt")
        );
        let Some(config_utils::DictConfig::Value(tasks)) =
            base.layer.tasks.clone()
        else {
            panic!("tasks")
        };
        let omni_configurations::TaskLayer::LongForm(task) = &tasks["t"] else {
            panic!("long form")
        };
        let out = task
            .cache
            .clone()
            .unwrap()
            .output
            .unwrap()
            .files
            .unwrap()
            .normalize();
        assert_eq!(out.include[0].path().unwrap(), root.join("shared/out"));

        let app = layers
            .iter()
            .find(|l| l.layer.name.as_deref() == Some("app"))
            .unwrap();
        assert_eq!(app.dir, OmniPath::from(root.join("app")));
        assert_eq!(
            app.layer.extends,
            Some(SingleOrMany::Single(OmniPath::from(
                root.join("shared/base.omni.yaml")
            )))
        );
    }

    #[tokio::test]
    async fn extends_outside_the_workspace_is_rejected() {
        let dir = tmp();
        let root = dir.path().join("ws");
        let app = write(
            &root,
            "app/project.omni.yaml",
            "name: app\nextends: ../../outside.yaml\n",
        );

        let err = ProjectConfigLoader::new(&RealSys, root.as_path())
            .load_project_layers(&[app])
            .await
            .unwrap_err();

        assert_eq!(
            err.kind(),
            ProjectConfigLoaderErrorKind::ExtendedProjectConfigOutsideWorkspace
        );
    }

    #[tokio::test]
    async fn missing_extendee_is_reported_with_its_extender() {
        let dir = tmp();
        let root = dir.path();
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../nope.yaml\n",
        );

        let err = ProjectConfigLoader::new(&RealSys, root)
            .load_project_layers(&[app])
            .await
            .unwrap_err();

        assert_eq!(
            err.kind(),
            ProjectConfigLoaderErrorKind::ExtendedProjectConfigDoesNotExist
        );
    }

    #[tokio::test]
    async fn the_eager_entry_point_requires_a_name() {
        let dir = tmp();
        let root = dir.path();
        let file = write(root, "app/project.omni.yaml", "base: true\n");

        let err = ProjectConfigLoader::new(&RealSys, root)
            .load_project_configs(&[file])
            .await
            .unwrap_err();

        assert_eq!(
            err.kind(),
            ProjectConfigLoaderErrorKind::MissingProjectName
        );
    }

    async fn compose(root: &Path, files: &[PathBuf]) -> Vec<FinalizedProject> {
        ProjectConfigLoader::new(&RealSys, root)
            .load_project_configs(files)
            .await
            .unwrap()
            .projects
    }

    fn named<'a>(
        projects: &'a [FinalizedProject],
        name: &str,
    ) -> &'a FinalizedProject {
        projects.iter().find(|p| p.config.name == name).unwrap()
    }

    fn deps(p: &FinalizedProject) -> Vec<String> {
        p.config.dependencies.clone().into_vec_inner()
    }

    #[tokio::test]
    async fn base_replace_then_child_append_replays_in_order() {
        let dir = tmp();
        let root = dir.path();
        write(
            root,
            "lib/a.yaml",
            "name: a\nbase: true\ndependencies: [x]\n",
        );
        write(
            root,
            "lib/b.yaml",
            "name: b\nbase: true\nextends: a.yaml\ndependencies:\n  replace: [base]\n",
        );
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../lib/b.yaml\ndependencies:\n  append: [child]\n",
        );

        let projects = compose(root, &[app]).await;

        assert_eq!(deps(named(&projects, "app")), ["base", "child"]);
    }

    #[tokio::test]
    async fn multiple_bases_apply_in_order_and_diamond_ancestor_applies_once() {
        let dir = tmp();
        let root = dir.path();
        write(
            root,
            "lib/top.yaml",
            "name: top\nbase: true\ndependencies: [top]\n",
        );
        write(
            root,
            "lib/l.yaml",
            "name: l\nbase: true\nextends: top.yaml\ndependencies:\n  append: [l]\n",
        );
        write(
            root,
            "lib/r.yaml",
            "name: r\nbase: true\nextends: top.yaml\ndependencies:\n  append: [r]\n",
        );
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: [../lib/l.yaml, ../lib/r.yaml]\n",
        );

        let projects = compose(root, &[app]).await;

        let d = deps(named(&projects, "app"));
        assert_eq!(d.iter().filter(|x| *x == "top").count(), 1, "{d:?}");
        assert!(d.contains(&"l".to_string()) && d.contains(&"r".to_string()));
    }

    #[tokio::test]
    async fn explicit_values_survive_a_child_that_omits_them() {
        let dir = tmp();
        let root = dir.path();
        write(
            root,
            "lib/base.yaml",
            "name: base\nbase: true\ncache:\n  output:\n    logs: false\ntasks:\n  t:\n    exec: x\n",
        );
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../lib/base.yaml\ncache:\n  key:\n    env: [E]\ntasks:\n  t:\n    cache:\n      key:\n        env: [T]\n",
        );

        let projects = compose(root, &[app]).await;

        let app = named(&projects, "app");
        assert!(!app.config.cache.output.logs);
        assert!(!app.task_cache["t"].output.logs);
    }

    #[tokio::test]
    async fn an_explicit_default_in_the_child_wins() {
        let dir = tmp();
        let root = dir.path();
        write(
            root,
            "lib/base.yaml",
            "name: base\nbase: true\ncache:\n  output:\n    logs: false\n",
        );
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../lib/base.yaml\ncache:\n  output:\n    logs: true\n",
        );

        let projects = compose(root, &[app]).await;

        assert!(named(&projects, "app").config.cache.output.logs);
    }

    #[tokio::test]
    async fn defaults_apply_only_after_composition() {
        let dir = tmp();
        let root = dir.path();
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\ntasks:\n  t: echo\n",
        );

        let projects = compose(root, &[app]).await;

        let app = named(&projects, "app");
        assert!(app.task_cache["t"].output.logs);
        assert!(!app.config.base);
    }

    #[tokio::test]
    async fn a_child_may_take_its_name_from_a_base() {
        let dir = tmp();
        let root = dir.path();
        write(root, "lib/base.yaml", "name: shared\nbase: true\n");
        let app =
            write(root, "app/project.omni.yaml", "extends: ../lib/base.yaml\n");

        let projects = compose(root, &[app]).await;

        assert!(projects.iter().any(|p| p.config.name == "shared"));
    }

    #[tokio::test]
    async fn a_project_without_any_name_fails_at_finalization() {
        let dir = tmp();
        let root = dir.path();
        let app = write(root, "app/project.omni.yaml", "description: d\n");

        let err = ProjectConfigLoader::new(&RealSys, root)
            .load_project_configs(&[app])
            .await
            .unwrap_err();

        assert_eq!(
            err.kind(),
            ProjectConfigLoaderErrorKind::MissingProjectName
        );
    }

    #[tokio::test]
    async fn base_is_declaration_local() {
        let dir = tmp();
        let root = dir.path();
        write(root, "lib/base.yaml", "name: base\nbase: true\n");
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\nextends: ../lib/base.yaml\n",
        );

        let projects = compose(root, &[app]).await;

        assert!(!named(&projects, "app").config.base);
        assert!(named(&projects, "base").config.base);
    }

    #[tokio::test]
    async fn task_extension_keeps_omitted_fields_absent() {
        let dir = tmp();
        let root = dir.path();
        let app = write(
            root,
            "app/project.omni.yaml",
            "name: app\ncache:\n  output:\n    logs: false\ntasks:\n  tpl:\n    base: true\n    exec: x\n  t:\n    extends: tpl\n",
        );

        let projects = compose(root, &[app]).await;

        let app = named(&projects, "app");
        assert!(!app.task_cache["t"].output.logs);
        assert!(!app.config.tasks.as_map().contains_key("tpl"));
    }
}
