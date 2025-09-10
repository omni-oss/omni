use std::{collections::HashMap, fs, path::Path};

use config_utils::{DictConfig, ListConfig, Replace};
use derive_builder::Builder;
use omni_configurations::ProjectConfiguration;
use omni_types::OmniPath;

use crate::{
    CacheConfigurationGenerator, CacheConfigurationGeneratorBuilder,
    MetaConfigurationGenerator, MetaConfigurationGeneratorBuilder,
    ProjectEnvConfigurationGenerator, ProjectEnvConfigurationGeneratorBuilder,
    TaskGenerator, TaskGeneratorBuilder,
};

#[derive(Debug, Builder, Clone)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct ProjectGenerator {
    pub(crate) name: String,
    #[builder(default)]
    base: bool,
    #[builder(setter(custom), default)]
    dependencies: Vec<String>,
    #[builder(setter(custom), default)]
    tasks: HashMap<String, TaskGenerator>,
    #[builder(default)]
    extends: Vec<String>,
    #[builder(default)]
    description: Option<String>,
    #[builder(default, setter(custom))]
    cache: CacheConfigurationGenerator,
    #[builder(default, setter(custom))]
    meta: MetaConfigurationGenerator,
    #[builder(default, setter(custom))]
    env: ProjectEnvConfigurationGenerator,
    #[builder(default = 2)]
    folder_nesting: usize,
    #[builder(default = 5)]
    leaf_folder_count: usize,
    #[builder(default = 10)]
    file_count_per_leaf_folder: usize,
    #[builder(default = String::from("txt"))]
    file_extension: String,
    #[builder(default = String::from("File context %i%"))]
    file_content: String,
}

impl ProjectGeneratorBuilder {
    pub fn dependency(&mut self, dependency: impl Into<String>) -> &mut Self {
        if let Some(dependencies) = &mut self.dependencies {
            dependencies.push(dependency.into());
        } else {
            self.dependencies = Some(vec![dependency.into()]);
        }

        self
    }

    pub fn task(
        &mut self,
        name: impl Into<String>,
        f: impl FnOnce(&mut TaskGeneratorBuilder) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        let mut task = TaskGeneratorBuilder::default();
        f(&mut task)?;

        if let Some(tasks) = &mut self.tasks {
            tasks.insert(name.into(), task.build()?);
        } else {
            let mut tasks = HashMap::with_capacity(1);

            tasks.insert(name.into(), task.build()?);

            self.tasks = Some(tasks);
        }

        Ok(self)
    }

    pub fn cache(
        &mut self,
        f: impl FnOnce(&mut CacheConfigurationGeneratorBuilder) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        let mut cache = CacheConfigurationGeneratorBuilder::default();
        f(&mut cache)?;
        self.cache = Some(cache.build()?);

        Ok(self)
    }

    pub fn meta(
        &mut self,
        f: impl FnOnce(&mut MetaConfigurationGeneratorBuilder) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        let mut meta = MetaConfigurationGeneratorBuilder::default();
        f(&mut meta)?;
        self.meta = Some(meta.build()?);

        Ok(self)
    }

    pub fn env(
        &mut self,
        f: impl FnOnce(
            &mut ProjectEnvConfigurationGeneratorBuilder,
        ) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        let mut env = ProjectEnvConfigurationGeneratorBuilder::default();
        f(&mut env)?;
        self.env = Some(env.build()?);

        Ok(self)
    }
}

impl ProjectGenerator {
    pub fn builder() -> ProjectGeneratorBuilder {
        ProjectGeneratorBuilder::default()
    }
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

        let file = std::io::BufWriter::new(file);
        serde_yml::to_writer(file, &project)?;

        generate_files(
            project_dir.join("src"),
            self.folder_nesting,
            self.leaf_folder_count,
            self.file_count_per_leaf_folder,
            &self.file_extension,
            &self.file_content,
        )?;

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
