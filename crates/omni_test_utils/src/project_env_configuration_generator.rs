use std::{collections::HashMap, path::PathBuf};

use config_utils::{DictConfig, ListConfig, Replace};
use derive_builder::Builder;
use omni_cli_core::configurations::ProjectEnvConfiguration;

#[derive(Debug, Clone, Default, Builder)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct ProjectEnvConfigurationGenerator {
    #[builder(default)]
    vars: HashMap<String, String>,
    #[builder(default)]
    files: Vec<String>,
}

impl ProjectEnvConfigurationGeneratorBuilder {
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

impl ProjectEnvConfigurationGenerator {
    pub fn builder() -> ProjectEnvConfigurationGeneratorBuilder {
        ProjectEnvConfigurationGeneratorBuilder::default()
    }
}

impl ProjectEnvConfigurationGenerator {
    pub fn generate(&self) -> ProjectEnvConfiguration {
        ProjectEnvConfiguration {
            files: ListConfig::append(
                self.files
                    .iter()
                    .map(|env| Replace::new(PathBuf::from(env)))
                    .collect(),
            ),
            vars: DictConfig::value(
                self.vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), Replace::new(v.to_string())))
                    .collect(),
            ),
        }
    }
}
