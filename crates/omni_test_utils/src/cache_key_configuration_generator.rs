use std::path::Path;

use config_utils::{ListConfig, Replace};
use omni_configurations::CacheKeyConfiguration;
use omni_types::OmniPath;

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct CacheKeyConfigurationGenerator {
    defaults: Option<bool>,
    #[builder(default)]
    env: Vec<String>,
    #[builder(default)]
    files: Vec<String>,
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
                    .map(|file| OmniPath::new(Path::new(file)))
                    .collect(),
            ),
        }
    }
}
