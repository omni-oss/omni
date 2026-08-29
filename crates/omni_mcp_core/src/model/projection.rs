use omni_api::BackupHandling;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionSyncParams {
    /// Compute the plan without touching the filesystem.
    #[serde(default)]
    pub dry_run: bool,
    /// Re-apply and repair every link even when its pin is unchanged.
    #[serde(default)]
    pub force: bool,
    /// Re-resolve mutable git revisions (e.g. branches) before applying.
    #[serde(default)]
    pub update: bool,
    /// Limit the pass to the projection source with this id.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionStatusParams {
    /// Include every recorded link, not just the summary counts.
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionUnlinkParams {
    /// The id of the projection source to tear down.
    pub id: String,
    /// How to dispose of each removed link's recorded backup: `leave`
    /// (default), `clean`, or `restore`. Absent means leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_handling: Option<BackupHandling>,
}

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionPruneParams {
    /// Report what would be pruned without removing anything.
    #[serde(default)]
    pub dry_run: bool,
}
