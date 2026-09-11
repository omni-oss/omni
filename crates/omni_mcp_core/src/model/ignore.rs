use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreSyncParams {
    /// Print the block that would be written without touching any file.
    #[serde(default)]
    pub dry_run: bool,
    /// Report staleness without writing. Fails when any file is missing the
    /// block or carries an out-of-date one.
    #[serde(default)]
    pub check: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreCleanParams {}
