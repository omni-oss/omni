use std::collections::HashMap;

use config_utils::{DictConfig, Replace};
use derive_builder::Builder;
use omni_cli_core::configurations::TaskEnvConfiguration;

#[derive(Debug, Clone, Default, Builder)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct TaskEnvConfigurationGenerator {
    #[builder(default)]
    vars: HashMap<String, String>,
}

impl TaskEnvConfigurationGeneratorBuilder {
    pub fn var(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        if let Some(vars) = &mut self.vars {
            vars.insert(key.into(), value.into());
        } else {
            self.vars = Some(HashMap::from([(key.into(), value.into())]));
        }

        self
    }
}

impl TaskEnvConfigurationGenerator {
    pub fn builder() -> TaskEnvConfigurationGeneratorBuilder {
        TaskEnvConfigurationGeneratorBuilder::default()
    }
}

impl TaskEnvConfigurationGenerator {
    pub fn generate(&self) -> TaskEnvConfiguration {
        TaskEnvConfiguration {
            vars: Some(DictConfig::value(
                self.vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), Replace::new(v.to_string())))
                    .collect(),
            )),
        }
    }
}
