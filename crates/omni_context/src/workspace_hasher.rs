use std::{collections::HashSet, path::Path};

use derive_new::new;
use omni_collector::{CollectConfig, Collector, CollectorSys, ProjectTaskInfo};
use omni_hasher::{
    Hasher as _,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::Hash,
};
use omni_types::OmniPath;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::project_data_extractor::ProjectDataExtractions;

#[derive(Debug, Clone, new)]
pub struct WorkspaceHasher<'a, TSys: CollectorSys + Clone> {
    root_dir: &'a Path,
    cache_dir: &'a Path,
    sys: &'a TSys,
}

impl<'a, TSys: CollectorSys + Clone> WorkspaceHasher<'a, TSys> {
    pub async fn hash(
        &self,
        seed: Option<&str>,
        extracted_data: &ProjectDataExtractions,
    ) -> Result<DefaultHash, WorkspaceHasherError> {
        let projects = extracted_data.projects.as_slice();

        let seed = seed.unwrap_or("DEFAULT_SEED");
        let seed = DefaultHasher::hash(seed.as_bytes());

        let mut task_infos_tmp = Vec::with_capacity(projects.len());

        struct ProjectTaskInfoTmp<'a> {
            project_name: &'a str,
            project_dir: &'a Path,
            task_name: &'a str,
            task_command: &'a str,
            input_files: Vec<OmniPath>,
            env_vars: maps::Map<String, String>,
            input_env_keys: Vec<String>,
        }

        for project in projects {
            let task_prefix = format!("{}#", project.name);
            let tasks_keys = extracted_data
                .cache_infos
                .keys()
                .filter(|k| k.starts_with(&task_prefix))
                .collect::<Vec<_>>();

            let mut input_files = HashSet::new();

            for key in tasks_keys {
                let ci =
                    extracted_data.cache_infos[key].key_input_files.clone();
                input_files.extend(ci);
            }

            let input_files = input_files.into_iter().collect::<Vec<_>>();

            task_infos_tmp.push(ProjectTaskInfoTmp {
                project_dir: &project.dir,
                project_name: &project.name,
                task_name: "temp",
                task_command: "",
                input_files,
                env_vars: maps::map![],
                input_env_keys: vec![],
            });
        }

        let task_infos = task_infos_tmp
            .iter()
            .map(|p| ProjectTaskInfo {
                project_dir: p.project_dir,
                project_name: p.project_name,
                task_name: p.task_name,
                task_command: p.task_command,
                input_files: &p.input_files,
                output_files: &[],
                dependency_hashes: &[],
                env_vars: &p.env_vars,
                input_env_keys: &p.input_env_keys,
            })
            .collect::<Vec<_>>();

        let mut collected =
            Collector::new(&self.root_dir, &self.cache_dir, self.sys.clone())
                .collect(
                    &task_infos,
                    &CollectConfig {
                        input_files: true,
                        output_files: false,
                        hashes: true,
                        cache_output_dirs: false,
                    },
                )
                .await?;

        // hash deterministically by sorting by hash
        collected.sort_by_key(|c| c.hash);

        let mut hash = Hash::<DefaultHasher>::new(seed);

        for collected in collected {
            hash.combine_in_place(
                collected.hash.as_ref().expect("should be some"),
            );
        }

        Ok(hash.to_inner())
    }

    pub async fn hash_string(
        &self,
        seed: Option<&str>,
        extracted_data: &ProjectDataExtractions,
    ) -> Result<String, WorkspaceHasherError> {
        Ok(
            bs58::encode(self.hash(seed, extracted_data).await?.as_ref())
                .into_string(),
        )
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct WorkspaceHasherError {
    #[source]
    inner: WorkspaceHasherErrorInner,
    kind: WorkspaceHasherErrorKind,
}

impl WorkspaceHasherError {
    pub fn kind(&self) -> WorkspaceHasherErrorKind {
        self.kind
    }
}

impl<T: Into<WorkspaceHasherErrorInner>> From<T> for WorkspaceHasherError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(WorkspaceHasherErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
enum WorkspaceHasherErrorInner {
    #[error(transparent)]
    Collector(#[from] omni_collector::Error),
}
