use std::path::Path;

use omni_api::{ProjectListEntry, ProjectSelector};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListParams {
    /// Also list base templates (configs with `base: true`).
    #[serde(default)]
    pub include_base: bool,
    /// Also list configs that are only reached through `extends`.
    #[serde(default)]
    pub include_extended: bool,
    /// List every loaded project configuration.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListResult {
    /// Names of the listed configs.
    pub projects: Vec<String>,
    /// The listed configs with their workspace-relative path and kind.
    pub entries: Vec<ProjectListEntry>,
}

/// Selects a project configuration. Exactly one of `name` and `path` is
/// required.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectConfigParams {
    /// Name of the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Path to the project configuration file. Relative paths are resolved
    /// against the workspace root, so a `path` from `project_list` entries
    /// can be passed unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl<'de> Deserialize<'de> for ProjectConfigParams {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            path: Option<String>,
        }

        let Raw { name, path } = Raw::deserialize(deserializer)?;
        if name.is_some() == path.is_some() {
            return Err(D::Error::custom(
                "exactly one of `name` or `path` is required",
            ));
        }

        Ok(Self { name, path })
    }
}

impl ProjectConfigParams {
    pub(crate) fn into_selector(
        self,
        workspace_root: &Path,
    ) -> eyre::Result<ProjectSelector> {
        match (self.name, self.path) {
            (Some(name), None) => Ok(ProjectSelector::Name(name)),
            (None, Some(path)) => {
                Ok(ProjectSelector::Path(workspace_root.join(path)))
            }
            (Some(_), Some(_)) | (None, None) => {
                Err(eyre::eyre!("exactly one of `name` or `path` is required"))
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectConfigResult {
    pub name: String,
    pub dir: String,
    pub description: Option<String>,
    pub tasks: Vec<TaskSummary>,
    /// Non-fatal notes about the selection, such as other configs sharing
    /// the requested name.
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskSummary {
    pub name: String,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> Result<ProjectConfigParams, String> {
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    #[test]
    fn project_config_params_accept_exactly_one_selector() {
        let by_name = parse(json!({ "name": "app" })).unwrap();
        assert_eq!(
            by_name.into_selector(Path::new("/ws")).unwrap(),
            ProjectSelector::Name("app".to_string())
        );

        let by_path =
            parse(json!({ "path": "apps/app/project.omni.yaml" })).unwrap();
        assert_eq!(
            by_path.into_selector(Path::new("/ws")).unwrap(),
            ProjectSelector::Path(
                Path::new("/ws").join("apps/app/project.omni.yaml")
            )
        );
    }

    #[test]
    fn project_config_params_reject_both_or_neither_selector() {
        for value in [
            json!({}),
            json!({ "name": "app", "path": "apps/app/project.omni.yaml" }),
        ] {
            let error = parse(value.clone()).expect_err("should be rejected");
            assert!(
                error.contains("exactly one of `name` or `path`"),
                "{value}: {error}"
            );
        }
    }

    #[test]
    fn project_list_params_default_when_absent() {
        let params: ProjectListParams =
            serde_json::from_value(json!({})).unwrap();
        assert!(!params.include_base);
        assert!(!params.include_extended);
        assert!(!params.all);
    }
}
