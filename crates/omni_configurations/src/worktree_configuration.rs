use std::path::Path;

use serde::{Deserialize, Deserializer};
use system_traits::FsRead;

use crate::LoadConfigError;

/// Machine-local cache policy. Missing fields inherit; explicit nulls do not.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeConfiguration {
    #[serde(default, deserialize_with = "present")]
    pub cache: Option<WorktreeCacheConfiguration>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeCacheConfiguration {
    #[serde(default, deserialize_with = "present")]
    pub dir: Option<String>,
}

fn present<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
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
        // mean inheritance, unlike explicit null values for cache or cache.dir.
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
    fn missing_values_inherit_but_nulls_and_unknown_fields_fail() {
        for value in ["{}", r#"{"cache":{}}"#] {
            let config: WorktreeConfiguration =
                serde_json::from_str(value).unwrap();
            assert!(config.cache.and_then(|cache| cache.dir).is_none());
        }
        for value in [
            r#"{"cache":null}"#,
            r#"{"cache":{"dir":null}}"#,
            r#"{"cache":{"directory":"cache"}}"#,
            r#"{"extends":"other"}"#,
        ] {
            assert!(
                serde_json::from_str::<WorktreeConfiguration>(value).is_err(),
                "{value}"
            );
        }
    }
}
