use std::path::Path;

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use garde::Validate;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

use crate::{
    CacheConfiguration, MetaConfiguration,
    utils::{self, fs::LoadConfigError, list_config_default},
};

use super::TaskConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Merge, Validate,
)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfiguration {
    #[serde(default, skip)]
    pub file: OmniPath,

    #[serde(default, skip)]
    pub dir: OmniPath,

    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub base: bool,

    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub extends: Vec<OmniPath>,

    #[merge(strategy = config_utils::replace)]
    pub name: String,

    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default = "list_config_default::<Replace<String>>")]
    pub dependencies: ListConfig<Replace<String>>,

    #[serde(default)]
    pub env: ProjectEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,

    #[serde(default)]
    #[merge(strategy = merge::option::recurse)]
    pub output_logs: Option<omni_task_output_logs::OutputLogsConfiguration>,

    #[serde(default)]
    pub meta: MetaConfiguration,

    #[serde(default)]
    pub tasks: DictConfig<TaskConfiguration>,
}

impl omni_core::ExtensionGraphNode for ProjectConfiguration {
    type Id = OmniPath;

    fn id(&self) -> &Self::Id {
        &self.file
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        &self.extends
    }
}

impl ProjectConfiguration {
    pub async fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
    }
}

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Merge,
    Default,
    Validate,
)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct ProjectEnvConfiguration {
    #[serde(default)]
    pub vars: DictConfig<Replace<String>>,
}

#[cfg(test)]
mod tests {
    use omni_task_output_logs::{
        LogsDisplay, OutputLogsConfiguration, OutputLogsSplit,
    };

    use super::*;

    #[test]
    fn test_merge_output_logs_project_to_task_semantics() {
        let mut base = ProjectConfiguration {
            file: OmniPath::default(),
            dir: OmniPath::default(),
            base: false,
            extends: vec![],
            name: "base".to_string(),
            description: None,
            dependencies: list_config_default(),
            env: ProjectEnvConfiguration::default(),
            cache: CacheConfiguration::default(),
            output_logs: Some(OutputLogsConfiguration::Uniform(
                LogsDisplay::Failed,
            )),
            meta: MetaConfiguration::default(),
            tasks: DictConfig::default(),
        };

        let derived = ProjectConfiguration {
            file: OmniPath::default(),
            dir: OmniPath::default(),
            base: false,
            extends: vec![],
            name: "derived".to_string(),
            description: None,
            dependencies: list_config_default(),
            env: ProjectEnvConfiguration::default(),
            cache: CacheConfiguration::default(),
            output_logs: Some(OutputLogsConfiguration::Split(
                OutputLogsSplit {
                    new: Some(LogsDisplay::All),
                    cached: None,
                },
            )),
            meta: MetaConfiguration::default(),
            tasks: DictConfig::default(),
        };

        base.merge(derived);

        assert_eq!(
            base.output_logs.unwrap().normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::Failed))
        );
    }

    #[test]
    fn test_merge_output_logs_none_keeps_base() {
        let mut base = ProjectConfiguration {
            file: OmniPath::default(),
            dir: OmniPath::default(),
            base: false,
            extends: vec![],
            name: "base".to_string(),
            description: None,
            dependencies: list_config_default(),
            env: ProjectEnvConfiguration::default(),
            cache: CacheConfiguration::default(),
            output_logs: Some(OutputLogsConfiguration::Uniform(
                LogsDisplay::All,
            )),
            meta: MetaConfiguration::default(),
            tasks: DictConfig::default(),
        };

        let derived = ProjectConfiguration {
            output_logs: None,
            ..base.clone()
        };

        base.merge(derived);

        assert_eq!(
            base.output_logs.unwrap().normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::All))
        );
    }

    #[test]
    fn test_schema_contains_output_logs() {
        let schema = schemars::schema_for!(ProjectConfiguration);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("output_logs"));
    }

    #[test]
    fn test_deserialize_scalar_and_split_output_logs() {
        let scalar: ProjectConfiguration =
            serde_json::from_str(r#"{"name":"p","output_logs":"never"}"#)
                .unwrap();
        assert_eq!(
            scalar.output_logs.unwrap().normalized(),
            (Some(LogsDisplay::Never), Some(LogsDisplay::Never))
        );

        let split: ProjectConfiguration = serde_json::from_str(
            r#"{"name":"p","output_logs":{"new":"all","cached":"never"}}"#,
        )
        .unwrap();
        assert_eq!(
            split.output_logs.unwrap().normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::Never))
        );
    }
}
