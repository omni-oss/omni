use std::collections::BTreeMap;

use config_utils::{DictConfig, Replace};
use omni_configurations::TaskEnvConfiguration;

/// Task-level env configuration generator.
///
/// Uses a [`BTreeMap`] so vars serialize in a stable, sorted order for
/// deterministic output.
#[derive(Debug, Clone, Default, bon::Builder)]
pub struct TaskEnvConfigurationGenerator {
    #[builder(default)]
    vars: BTreeMap<String, String>,
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
