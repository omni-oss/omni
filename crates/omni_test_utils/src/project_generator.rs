use std::{collections::HashMap, fs, path::Path};

use config_utils::{DictConfig, ListConfig, Replace};
use derive_builder::Builder;
use omni_cli_core::configurations::ProjectConfiguration;
use omni_types::OmniPath;

use crate::{
    CacheConfigurationGenerator, CacheConfigurationGeneratorBuilder,
    CacheConfigurationGeneratorBuilderError, MetaConfigurationGenerator,
    MetaConfigurationGeneratorBuilder, MetaConfigurationGeneratorBuilderError,
    ProjectEnvConfigurationGenerator, ProjectEnvConfigurationGeneratorBuilder,
    ProjectEnvConfigurationGeneratorBuilderError, TaskGenerator,
    TaskGeneratorBuilder, TaskGeneratorBuilderError,
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
        f: impl FnOnce(&mut TaskGeneratorBuilder) -> &mut TaskGeneratorBuilder,
    ) -> Result<&mut Self, TaskGeneratorBuilderError> {
        let mut task = TaskGeneratorBuilder::default();
        f(&mut task);

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
        f: impl FnOnce(
            &mut CacheConfigurationGeneratorBuilder,
        ) -> &mut CacheConfigurationGeneratorBuilder,
    ) -> Result<&mut Self, CacheConfigurationGeneratorBuilderError> {
        let mut cache = CacheConfigurationGeneratorBuilder::default();
        f(&mut cache);
        self.cache = Some(cache.build()?);

        Ok(self)
    }

    pub fn meta(
        &mut self,
        f: impl FnOnce(
            &mut MetaConfigurationGeneratorBuilder,
        ) -> &mut MetaConfigurationGeneratorBuilder,
    ) -> Result<&mut Self, MetaConfigurationGeneratorBuilderError> {
        let mut meta = MetaConfigurationGeneratorBuilder::default();
        f(&mut meta);
        self.meta = Some(meta.build()?);

        Ok(self)
    }

    pub fn env(
        &mut self,
        f: impl FnOnce(
            &mut ProjectEnvConfigurationGeneratorBuilder,
        ) -> &mut ProjectEnvConfigurationGeneratorBuilder,
    ) -> Result<&mut Self, ProjectEnvConfigurationGeneratorBuilderError> {
        let mut env = ProjectEnvConfigurationGeneratorBuilder::default();
        f(&mut env);
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

        if !fs::exists(project_dir)? {
            fs::create_dir_all(project_dir)?;
        }

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(project_dir.join("project.omni.yml"))?;

        let file = std::io::BufWriter::new(file);
        serde_yml::to_writer(file, &project)?;

        Ok(())
    }
}
