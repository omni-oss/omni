use std::path::Path;

use serde::Deserialize;
use system_traits::FsRead;

use crate::LoadConfigError;

/// Machine-local cache policy. Missing and null fields both inherit.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeConfiguration {
    #[serde(default)]
    pub cache: Option<WorktreeCacheConfiguration>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeCacheConfiguration {
    #[serde(default)]
    pub dir: Option<String>,
}

impl WorktreeConfiguration {
    #[allow(clippy::result_large_err)]
    pub fn load(
        path: &Path,
        sys: &(impl FsRead + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        let content = sys
            .fs_read(path)
            .map_err(|error| (path.to_path_buf(), error))?;
        let format = omni_file_data_serde::ext_to_format(
            path.extension().unwrap_or_default(),
        )
        .map_err(|error| (path.to_path_buf(), error))?;
        // YAML has no empty-map spelling requirement. Empty/comment-only files
        // mean inheritance, as do null values for cache or cache.dir.
        if format == omni_file_data_serde::Format::Yaml
            && std::str::from_utf8(&content).is_ok_and(|text| {
                text.lines().all(|line| {
                    let line = line.trim();
                    line.is_empty() || line.starts_with('#')
                })
            })
        {
            return Ok(Self::default());
        }
        Ok(omni_file_data_serde::from_slice(&content, format)
            .map_err(|error| (path.to_path_buf(), error))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_null_values_inherit_but_invalid_fields_fail() {
        for value in [
            "{}",
            r#"{"cache":{}}"#,
            r#"{"cache":null}"#,
            r#"{"cache":{"dir":null}}"#,
        ] {
            let config: WorktreeConfiguration =
                serde_json::from_str(value).unwrap();
            assert!(config.cache.and_then(|cache| cache.dir).is_none());
        }
        for value in [
            r#"{"cache":{"directory":"cache"}}"#,
            r#"{"extends":"other"}"#,
            r#"{"cache":false}"#,
            r#"{"cache":{"dir":42}}"#,
            r#"{"cache":{"dir":[]}}"#,
        ] {
            assert!(
                serde_json::from_str::<WorktreeConfiguration>(value).is_err(),
                "{value}"
            );
        }
    }
}
