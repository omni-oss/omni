use derive_builder::Builder;
use omni_configurations::CacheConfiguration;

use crate::{
    CacheKeyConfigurationGenerator, CacheKeyConfigurationGeneratorBuilder,
};

#[derive(Debug, Clone, Builder, Default)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct CacheConfigurationGenerator {
    enabled: bool,
    #[builder(setter(custom), default)]
    key: CacheKeyConfigurationGenerator,
}

impl CacheConfigurationGeneratorBuilder {
    pub fn key(
        &mut self,
        f: impl FnOnce(
            &mut CacheKeyConfigurationGeneratorBuilder,
        ) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        let mut key = CacheKeyConfigurationGeneratorBuilder::default();
        f(&mut key)?;
        self.key = Some(key.build()?);

        Ok(self)
    }
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
