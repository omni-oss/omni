use derive_new::new;
use maps::Map;
use serde::{Deserialize, Serialize};

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

/// Pins keyed `kind -> locator -> selector`. The lockfile does not know the
/// set of kinds: the remote-source manager checks them after load.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LockfileDataV1_0_0 {
    pub sources: Map<String, Map<String, Map<String, PinLockData>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, new)]
#[serde(deny_unknown_fields)]
pub struct PinLockData {
    #[new(into)]
    pub pin: String,
}
