use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use derive_new::new;
use enum_map::enum_map;
use omni_configurations::{
    LoadConfigError, ProjectConfiguration, TaskConfiguration,
};
use omni_types::Root;
use path_clean::clean;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use thiserror::Error;

use crate::ContextSys;

#[derive(Debug, Clone, Copy, new)]
pub struct ProjectConfigLoader<'a, TSys: ContextSys> {
    #[new(into)]
    sys: &'a TSys,
    #[new(into)]
    root_dir: &'a Path,
}

impl<'a, TSys: ContextSys> ProjectConfigLoader<'a, TSys> {
    pub async fn load_project_configs(
        &self,
        project_paths: &[PathBuf],
    ) -> Result<Vec<ProjectConfiguration>, ProjectConfigLoaderError> {
        let mut project_configs = vec![];
        let mut loaded = HashSet::new();
        let mut to_process = project_paths.to_vec();
        to_process.sort();
        let root_dir = &self.root_dir;

        while let Some(conf) = to_process.pop() {
            let start_time = std::time::SystemTime::now();

            let mut project =
                ProjectConfiguration::load(&conf as &Path, self.sys).await?;

            let elapsed = start_time.elapsed().unwrap_or_default();
            trace::trace!(
                project_configuration = ?project,
                "loaded project configuration file {:?} in {} ms",
                conf,
                elapsed.as_millis()
            );

            project.file = conf.clone().into();
            let project_dir = conf.parent().ok_or_else(|| {
                ProjectConfigLoaderErrorInner::NoParentDirFoundForPath(
                    conf.clone(),
                )
            })?;
            project.dir = project_dir.into();
            loaded.insert(project.file.clone());

            let bases = enum_map! {
                Root::Workspace => root_dir,
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
                if dep.is_rooted() {
                    dep.resolve_in_place(&bases);
                }

                *dep = clean(
                    project_dir
                        .join(dep.path().expect("path should be resolved")),
                )
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

        Ok(project_configs)
    }
}

#[derive(Error, Debug)]
#[error(transparent)]
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

#[derive(Error, Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ProjectConfigLoaderErrorKind))]
pub(crate) enum ProjectConfigLoaderErrorInner {
    #[error(transparent)]
    LoadConfig(#[from] LoadConfigError),

    #[error("no parent dir found for path: {0}")]
    NoParentDirFoundForPath(PathBuf),
}
