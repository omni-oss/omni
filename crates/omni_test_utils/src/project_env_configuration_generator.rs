use std::collections::BTreeMap;

use config_utils::{DictConfig, Replace};
use omni_configurations::ProjectEnvConfiguration;

/// Project-level env configuration generator.
///
/// Uses a [`BTreeMap`] so vars serialize in a stable, sorted order for
/// deterministic output.
#[derive(Debug, Clone, Default, bon::Builder)]
pub struct ProjectEnvConfigurationGenerator {
    #[builder(default)]
    vars: BTreeMap<String, String>,
}

impl ProjectEnvConfigurationGenerator {
    pub fn generate(&self) -> ProjectEnvConfiguration {
        ProjectEnvConfiguration {
            vars: DictConfig::value(
                self.vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), Replace::new(v.to_string())))
                    .collect(),
            ),
        }
    }
}
