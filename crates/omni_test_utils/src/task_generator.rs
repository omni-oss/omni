use std::str::FromStr;

use config_utils::{ListConfig, Replace};
use omni_configurations::{
    TaskConfiguration, TaskConfigurationLongForm, TaskDependencyConfiguration,
};

use crate::{
    CacheConfigurationGenerator, MetaConfigurationGenerator,
    TaskEnvConfigurationGenerator, TaskOutputConfigurationGenerator,
};

#[derive(Debug, Clone, bon::Builder)]
pub struct TaskGenerator {
    #[builder(into)]
    command: String,
    #[builder(default)]
    cache: CacheConfigurationGenerator,
    #[builder(into)]
    description: Option<String>,
    #[builder(default)]
    env: TaskEnvConfigurationGenerator,
    #[builder(default)]
    meta: MetaConfigurationGenerator,
    #[builder(default)]
    dependencies: Vec<String>,
    #[builder(default)]
    output: TaskOutputConfigurationGenerator,
}

impl TaskGenerator {
    pub fn generate(&self) -> TaskConfiguration {
        let mut cache = self.cache.generate();
        cache.output = self.output.generate();

        TaskConfiguration::LongForm(Box::new(TaskConfigurationLongForm {
            exec: Some(Replace::new(self.command.clone())),
            cache,
            description: self.description.clone().map(Replace::new),
            env: self.env.generate(),
            meta: self.meta.generate(),
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
