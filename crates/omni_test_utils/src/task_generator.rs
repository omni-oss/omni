use std::str::FromStr;

use config_utils::{ListConfig, Replace};
use derive_builder::Builder;
use omni_configurations::{
    TaskConfiguration, TaskConfigurationLongForm, TaskDependencyConfiguration,
};

use crate::{
    CacheConfigurationGenerator, CacheConfigurationGeneratorBuilder,
    MetaConfigurationGenerator, MetaConfigurationGeneratorBuilder,
    TaskEnvConfigurationGenerator, TaskEnvConfigurationGeneratorBuilder,
    TaskOutputConfigurationGenerator, TaskOutputConfigurationGeneratorBuilder,
};

#[derive(Debug, Builder, Clone)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct TaskGenerator {
    command: String,
    #[builder(default, setter(custom))]
    cache: CacheConfigurationGenerator,
    #[builder(default)]
    description: Option<String>,
    #[builder(default, setter(custom))]
    env: TaskEnvConfigurationGenerator,
    #[builder(default, setter(custom))]
    meta: MetaConfigurationGenerator,
    #[builder(default)]
    dependencies: Vec<String>,
    #[builder(default, setter(custom))]
    output: TaskOutputConfigurationGenerator,
}

impl TaskGeneratorBuilder {
    pub fn cache(
        &mut self,
        f: impl FnOnce(&mut CacheConfigurationGeneratorBuilder),
    ) -> eyre::Result<&mut Self> {
        let mut cache = CacheConfigurationGeneratorBuilder::default();
        f(&mut cache);
        self.cache = Some(cache.build()?);

        Ok(self)
    }

    pub fn env(
        &mut self,
        f: impl FnOnce(
            &mut TaskEnvConfigurationGeneratorBuilder,
        ) -> &mut TaskEnvConfigurationGeneratorBuilder,
    ) -> eyre::Result<&mut Self> {
        let mut env = TaskEnvConfigurationGeneratorBuilder::default();
        f(&mut env);
        self.env = Some(env.build()?);

        Ok(self)
    }

    pub fn meta(
        &mut self,
        f: impl FnOnce(&mut MetaConfigurationGeneratorBuilder),
    ) -> eyre::Result<&mut Self> {
        let mut meta = MetaConfigurationGeneratorBuilder::default();
        f(&mut meta);
        self.meta = Some(meta.build()?);

        Ok(self)
    }

    pub fn output(
        &mut self,
        f: impl FnOnce(&mut TaskOutputConfigurationGeneratorBuilder),
    ) -> eyre::Result<&mut Self> {
        let mut output = TaskOutputConfigurationGeneratorBuilder::default();
        f(&mut output);
        self.output = Some(output.build()?);

        Ok(self)
    }

    pub fn dependency(&mut self, dependency: impl Into<String>) -> &mut Self {
        if let Some(dependencies) = &mut self.dependencies {
            dependencies.push(dependency.into());
        } else {
            self.dependencies = Some(vec![dependency.into()]);
        }

        self
    }
}

impl TaskGenerator {
    pub fn builder() -> TaskGeneratorBuilder {
        TaskGeneratorBuilder::default()
    }
}

impl TaskGenerator {
    pub fn generate(&self) -> TaskConfiguration {
        TaskConfiguration::LongForm(Box::new(TaskConfigurationLongForm {
            command: self.command.clone(),
            cache: self.cache.generate(),
            description: self.description.clone().map(|e| Replace::new(e)),
            env: self.env.generate(),
            meta: self.meta.generate(),
            output: self.output.generate(),
            dependencies: ListConfig::append(
                self.dependencies
                    .iter()
                    .map(|dependency| {
                        TaskDependencyConfiguration::from_str(dependency)
                            .expect("Can't parse dependency")
                    })
                    .collect(),
            ),
            ..Default::default()
        }))
    }
}
