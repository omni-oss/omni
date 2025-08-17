use std::path::Path;

use config_utils::{ListConfig, Replace};
use derive_builder::Builder;
use omni_cli_core::configurations::CacheKeyConfiguration;
use omni_types::OmniPath;

#[derive(Debug, Clone, Builder, Default)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct CacheKeyConfigurationGenerator {
    #[builder(default)]
    defaults: bool,
    #[builder(default)]
    env: Vec<String>,
    #[builder(default)]
    files: Vec<String>,
}

impl CacheKeyConfigurationGenerator {
    pub fn builder() -> CacheKeyConfigurationGeneratorBuilder {
        CacheKeyConfigurationGeneratorBuilder::default()
    }
}

impl CacheKeyConfigurationGenerator {
    pub fn generate(&self) -> CacheKeyConfiguration {
        CacheKeyConfiguration {
            defaults: self.defaults,
            env: ListConfig::append(
                self.env
                    .iter()
                    .map(|env| Replace::new(env.to_string()))
                    .collect(),
            ),
            files: ListConfig::append(
                self.files
                    .iter()
                    .map(|env| OmniPath::new(Path::new(env)))
                    .collect(),
            ),
        }
    }
}
