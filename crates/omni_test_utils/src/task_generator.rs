use std::str::FromStr;

use config_utils::ListConfig;
use derive_builder::Builder;
use omni_cli_core::configurations::{
    TaskConfiguration, TaskConfigurationLongForm, TaskDependencyConfiguration,
};

use crate::{
    CacheConfigurationGenerator, CacheConfigurationGeneratorBuilder,
    CacheConfigurationGeneratorBuilderError, MetaConfigurationGenerator,
    MetaConfigurationGeneratorBuilder, MetaConfigurationGeneratorBuilderError,
    TaskEnvConfigurationGenerator, TaskEnvConfigurationGeneratorBuilder,
    TaskEnvConfigurationGeneratorBuilderError,
    TaskOutputConfigurationGenerator, TaskOutputConfigurationGeneratorBuilder,
    TaskOutputConfigurationGeneratorBuilderError,
};

#[derive(Debug, Builder, Clone)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct TaskGenerator {
    command: String,
    #[builder(default, setter(custom))]
    cache: CacheConfigurationGenerator,
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
        f: impl FnOnce(
            &mut CacheConfigurationGeneratorBuilder,
        ) -> &mut CacheConfigurationGeneratorBuilder,
    ) -> Result<&mut Self, CacheConfigurationGeneratorBuilderError> {
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
    ) -> Result<&mut Self, TaskEnvConfigurationGeneratorBuilderError> {
        let mut env = TaskEnvConfigurationGeneratorBuilder::default();
        f(&mut env);
        self.env = Some(env.build()?);

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

    pub fn output(
        &mut self,
        f: impl FnOnce(
            &mut TaskOutputConfigurationGeneratorBuilder,
        ) -> &mut TaskOutputConfigurationGeneratorBuilder,
    ) -> Result<&mut Self, TaskOutputConfigurationGeneratorBuilderError> {
        let mut output = TaskOutputConfigurationGeneratorBuilder::default();
        f(&mut output);
        self.output = Some(output.build()?);

        Ok(self)
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
            description: self.description.clone(),
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
        }))
    }
}
