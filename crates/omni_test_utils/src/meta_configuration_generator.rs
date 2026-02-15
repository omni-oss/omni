use std::collections::HashMap;

use config_utils::{DictConfig, DynValue};
use derive_builder::Builder;
use omni_configurations::MetaConfiguration;

#[derive(Debug, Clone, Default, Builder)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct MetaConfigurationGenerator {
    #[builder(default, setter(custom))]
    values: HashMap<String, DynValue>,
}

impl MetaConfigurationGeneratorBuilder {
    pub fn value(
        &mut self,
        key: impl Into<String>,
        value: DynValue,
    ) -> &mut Self {
        if let Some(values) = &mut self.values {
            values.insert(key.into(), value);
        } else {
            self.values = Some(HashMap::from([(key.into(), value)]));
        }

        self
    }
}

impl MetaConfigurationGenerator {
    pub fn builder() -> MetaConfigurationGeneratorBuilder {
        MetaConfigurationGeneratorBuilder::default()
    }
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
