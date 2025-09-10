use config_utils::ListConfig;
use derive_builder::Builder;
use omni_configurations::TaskOutputConfiguration;
use omni_types::OmniPath;

#[derive(Debug, Builder, Clone, Default)]
#[builder(setter(into, strip_option), derive(Debug))]
pub struct TaskOutputConfigurationGenerator {
    #[builder(default)]
    logs: bool,
    #[builder(default)]
    files: Vec<String>,
}

impl TaskOutputConfigurationGeneratorBuilder {
    pub fn file(&mut self, file: impl Into<String>) -> &mut Self {
        if let Some(files) = &mut self.files {
            files.push(file.into());
        } else {
            self.files = Some(vec![file.into()]);
        }

        self
    }
}

impl TaskOutputConfigurationGenerator {
    pub fn builder() -> TaskOutputConfigurationGeneratorBuilder {
        TaskOutputConfigurationGeneratorBuilder::default()
    }
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
