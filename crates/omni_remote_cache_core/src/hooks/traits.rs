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

fn hex_prefix(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The id hooks see for `api_key`: 16 hex characters of its SHA-256.
///
/// This is unkeyed, so anyone who can read an id can test key guesses
/// offline. That is fine for long random keys. Use `api_key_id_keyed`
/// otherwise.
pub fn api_key_id(api_key: &str) -> String {
    use sha2::{Digest as _, Sha256};

    hex_prefix(&Sha256::digest(api_key.as_bytes()))
}

/// Like `api_key_id`, but an HMAC-SHA256 under `secret`. Without the secret an
/// id cannot be computed for a guessed key, so a leaked usage table does not
/// allow offline guessing. Changing the secret changes every id.
pub fn api_key_id_keyed(secret: &str, api_key: &str) -> String {
    use hmac::{Hmac, Mac as _};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts keys of any length");
    mac.update(api_key.as_bytes());
    hex_prefix(&mac.finalize().into_bytes())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_ids_depend_on_the_secret_and_never_equal_the_plain_id() {
        let plain = api_key_id("key");
        let a = api_key_id_keyed("secret-a", "key");
        let b = api_key_id_keyed("secret-b", "key");
        assert_eq!(a, api_key_id_keyed("secret-a", "key"), "deterministic");
        assert_ne!(a, b);
        assert_ne!(a, plain);
        assert_ne!(a, api_key_id_keyed("secret-a", "other"));
        assert_eq!(a.len(), 16);
    }
}
