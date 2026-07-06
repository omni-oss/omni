use config_utils::Replace;
use omni_configurations::CacheConfiguration;

use crate::CacheKeyConfigurationGenerator;

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct CacheConfigurationGenerator {
    #[builder(default)]
    enabled: bool,
    #[builder(default)]
    key: CacheKeyConfigurationGenerator,
}

impl CacheConfigurationGenerator {
    pub fn generate(&self) -> CacheConfiguration {
        CacheConfiguration {
            enabled: Some(Replace::new(self.enabled)),
            key: self.key.generate(),
            output: Default::default(),
        }
    }
}
