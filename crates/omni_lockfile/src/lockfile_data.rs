use derive_new::new;
use maps::Map;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "kebab-case")]
pub enum LockfileData {
    #[serde(rename = "1.0.0")]
    V1_0_0(LockfileDataV1_0_0),
}

impl Default for LockfileData {
    fn default() -> Self {
        Self::V1_0_0(LockfileDataV1_0_0::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockfileDataV1_0_0 {
    pub git: Map<Url, Map<String, GitRepoLockData>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, new)]
pub struct GitRepoLockData {
    #[new(into)]
    pub commit: String,
}
