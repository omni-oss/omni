use config_utils::ListConfig;
use omni_configurations::TaskOutputConfiguration;
use omni_types::OmniPath;

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct TaskOutputConfigurationGenerator {
    #[builder(default)]
    logs: bool,
    #[builder(default)]
    files: Vec<String>,
}

impl TaskOutputConfigurationGenerator {
    pub fn generate(&self) -> TaskOutputConfiguration {
        TaskOutputConfiguration {
            logs: self.logs,
            files: ListConfig::value(
                self.files
                    .iter()
                    .map(|file| OmniPath::new(file.to_string()))
                    .collect(),
            ),
        }
    }
}
