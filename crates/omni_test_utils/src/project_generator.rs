use std::{collections::BTreeMap, fs, path::Path};

use config_utils::{DictConfig, ListConfig, Replace};
use omni_configurations::ProjectConfiguration;
use omni_types::OmniPath;

use crate::{
    CacheConfigurationGenerator, MetaConfigurationGenerator,
    ProjectEnvConfigurationGenerator, TaskGenerator,
};

#[derive(Debug, Clone, bon::Builder)]
pub struct ProjectGenerator {
    #[builder(into)]
    pub(crate) name: String,
    #[builder(default)]
    base: bool,
    #[builder(default)]
    dependencies: Vec<String>,
    /// Tasks keyed by name. A [`BTreeMap`] keeps serialization order stable so
    /// the generated config is byte-identical across runs.
    #[builder(default)]
    tasks: BTreeMap<String, TaskGenerator>,
    #[builder(default)]
    extends: Vec<String>,
    #[builder(into)]
    description: Option<String>,
    #[builder(default)]
    cache: CacheConfigurationGenerator,
    #[builder(default)]
    meta: MetaConfigurationGenerator,
    #[builder(default)]
    env: ProjectEnvConfigurationGenerator,
    /// Extra files to write into the project directory, keyed by path relative
    /// to the project root (e.g. `run.sh`). Written verbatim; a [`BTreeMap`]
    /// keeps write order stable for deterministic output.
    #[builder(default)]
    extra_files: BTreeMap<String, String>,
    #[builder(default = 2)]
    folder_nesting: usize,
    #[builder(default = 5)]
    leaf_folder_count: usize,
    #[builder(default = 10)]
    file_count_per_leaf_folder: usize,
    #[builder(default = String::from("txt"), into)]
    file_extension: String,
    #[builder(default = String::from("File context %i%"), into)]
    file_content: String,
}

impl ProjectGenerator {
    pub fn generate(&self, project_dir: impl AsRef<Path>) -> eyre::Result<()> {
        let project_dir = project_dir.as_ref();

        if !fs::exists(project_dir)? {
            fs::create_dir_all(project_dir)?;
        }

        let project = ProjectConfiguration {
            dir: OmniPath::new(project_dir),
            file: OmniPath::new(project_dir.join("project.omni.yml")),
            base: self.base,
            cache: self.cache.generate(),
            output_logs: None,
            name: self.name.clone(),
            extends: self
                .extends
                .iter()
                .map(|extends| OmniPath::new(Path::new(extends)))
                .collect(),
            dependencies: ListConfig::append(
                self.dependencies
                    .iter()
                    .map(|dependency| Replace::new(dependency.clone()))
                    .collect(),
            ),
            description: self
                .description
                .as_ref()
                .map(|description| Replace::new(description.clone())),
            env: self.env.generate(),
            meta: self.meta.generate(),
            tasks: DictConfig::value(
                self.tasks
                    .iter()
                    .map(|(name, task)| (name.to_string(), task.generate()))
                    .collect(),
            ),
        };

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(project_dir.join("project.omni.yml"))?;

        let mut file = std::io::BufWriter::new(file);
        omni_file_data_serde::to_writer(
            &mut file,
            &project,
            omni_file_data_serde::Format::Yaml,
        )?;

        generate_files(
            project_dir.join("src"),
            self.folder_nesting,
            self.leaf_folder_count,
            self.file_count_per_leaf_folder,
            &self.file_extension,
            &self.file_content,
        )?;

        for (rel_path, contents) in &self.extra_files {
            let path = project_dir.join(rel_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, contents)?;
        }

        Ok(())
    }
}

fn generate_files(
    dir: impl AsRef<Path>,
    folder_nesting: usize,
    leaf_folder_count: usize,
    file_count_per_leaf_folder: usize,
    file_extension: impl AsRef<str>,
    file_content: impl AsRef<str>,
) -> eyre::Result<()> {
    let dir = dir.as_ref();
    let file_extension = file_extension.as_ref();
    let file_content = file_content.as_ref();

    let mut leaf_dirs = vec![];

    for i in 0..leaf_folder_count {
        if folder_nesting == 0 {
            leaf_dirs.push(dir.join(format!("leaf_{}", i)));
        } else {
            let nested_paths = (0..folder_nesting)
                .map(|j| {
                    if j == 0 {
                        format!("root_{}", i)
                    } else {
                        format!("nested_level_{}", j)
                    }
                })
                .collect::<Vec<_>>()
                .join("/");

            leaf_dirs.push(dir.join(nested_paths).join(format!("leaf_{}", i)));
        }
    }

    for l in &leaf_dirs {
        fs::create_dir_all(l)?;

        for i in 0..file_count_per_leaf_folder {
            let file = if file_extension.is_empty() {
                l.join(format!("file_{}.txt", i))
            } else {
                l.join(format!("file_{}.{}", i, file_extension))
            };

            if file_content.contains("%i%") {
                let content = file_content.replace("%i%", &i.to_string());

                fs::write(file, content)?;
            } else {
                fs::write(file, file_content)?;
            }
        }
    }

    Ok(())
}
