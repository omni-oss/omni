use derive_builder::Builder;
use omni_cli_core::configurations::CacheConfiguration;

use crate::CacheKeyConfigurationGenerator;

#[derive(Debug, Clone, Builder, Default)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct CacheConfigurationGenerator {
    enabled: bool,
    key: CacheKeyConfigurationGenerator,
}

impl CacheConfigurationGenerator {
    pub fn builder() -> CacheConfigurationGeneratorBuilder {
        CacheConfigurationGeneratorBuilder::default()
    }
}

impl CacheConfigurationGenerator {
    pub fn generate(&self) -> CacheConfiguration {
        CacheConfiguration {
            enabled: self.enabled,
            key: self.key.generate(),
        }
    }
}
