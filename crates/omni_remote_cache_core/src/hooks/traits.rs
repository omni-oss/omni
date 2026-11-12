use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Display,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOp {
    Get,
    GetMany,
    Head,
    HeadMany,
    Put,
    Delete,
}

/// What a hook is told about the operation. `digests` holds the one digest
/// for single-artifact operations and is empty for the list operations
/// (`GetMany`, `HeadMany`), which address a whole environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookContext {
    pub op: ArtifactOp,
    pub tenant: String,
    pub org: String,
    pub ws: String,
    pub env: String,
    pub digests: Vec<String>,
    /// An opaque, stable id for the presented API key (a truncated SHA-256
    /// fingerprint). It identifies the key in usage records without
    /// exposing it.
    pub api_key_id: String,
}

/// The id hooks see for `api_key`: 16 hex characters of its SHA-256.
pub fn api_key_id(api_key: &str) -> String {
    use sha2::{Digest as _, Sha256};

    Sha256::digest(api_key.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Veto { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum OutcomeKind {
    Hit,
    Miss,
    Stored,
    Deleted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub kind: OutcomeKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct HookError(#[from] pub eyre::Report);

#[async_trait]
pub trait ArtifactHooks: Send + Sync + 'static {
    /// Runs before the operation. May veto it.
    async fn before(
        &self,
        ctx: &HookContext,
    ) -> Result<HookDecision, HookError>;

    /// Observes the finished operation. Cannot change its result.
    async fn after(
        &self,
        ctx: &HookContext,
        outcome: &HookOutcome,
    ) -> Result<(), HookError>;
}

pub type DynArtifactHooks = Arc<dyn ArtifactHooks>;

pub type BoxTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Runs work after the response and provides a timer, since core is
/// runtime-agnostic (tokio natively, `waitUntil` on Workers).
pub trait HostRuntime: Send + Sync + 'static {
    fn spawn_detached(&self, task: BoxTask);
    fn sleep(&self, duration: Duration) -> BoxTask;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    FailOpen,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSettings {
    pub before_timeout: Duration,
    pub after_timeout: Duration,
    pub before_policy: FailurePolicy,
    /// Most `after` hooks in flight at once. Beyond it they are shed and
    /// logged, so slow hooks cannot starve request-path work.
    pub after_max_in_flight: usize,
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            before_timeout: Duration::from_secs(2),
            after_timeout: Duration::from_secs(10),
            before_policy: FailurePolicy::FailClosed,
            after_max_in_flight: 64,
        }
    }
}
