use std::path::Path;

use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsRead, FsReadAsync};

use crate::{LoadConfigError, utils};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Merge, Validate,
)]
#[garde(allow_unvalidated)]
pub struct RemoteCacheConfiguration {
    #[merge(strategy = config_utils::replace)]
    pub api_key: String,

    #[merge(strategy = config_utils::replace)]
    pub api_base_url: String,

    #[merge(strategy = config_utils::replace)]
    pub tenant_code: String,

    #[merge(strategy = config_utils::replace)]
    pub organization_code: String,

    #[merge(strategy = config_utils::replace)]
    pub workspace_code: String,

    #[merge(strategy = config_utils::replace_if_some)]
    pub environment_code: Option<String>,
}

impl RemoteCacheConfiguration {
    pub async fn load_async<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
    }

    pub fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsRead + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config(path, sys)
    }
}
