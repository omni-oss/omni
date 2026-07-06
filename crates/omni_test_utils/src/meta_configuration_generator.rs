use std::collections::BTreeMap;

use config_utils::{DictConfig, DynValue};
use omni_configurations::MetaConfiguration;

/// Meta configuration generator.
///
/// Uses a [`BTreeMap`] so entries serialize in a stable, sorted order,
/// guaranteeing byte-identical output across runs.
#[derive(Debug, Clone, Default, bon::Builder)]
pub struct MetaConfigurationGenerator {
    #[builder(default)]
    values: BTreeMap<String, DynValue>,
}

impl MetaConfigurationGenerator {
    pub fn generate(&self) -> MetaConfiguration {
        MetaConfiguration(DictConfig::value(
            self.values
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        ))
    }
}
