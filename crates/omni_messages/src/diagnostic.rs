use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A structured diagnostic message emitted by business logic in place of
/// direct `log::*` / `println!` calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub message: String,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub target: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display,
)]
pub enum DiagnosticLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Shared diagnostic channel used by both execution and generator subscribers.
///
/// Extracted as a supertrait so the [`diagnostic!`] macro and call sites
/// can use a single unambiguous method, regardless of whether the subscriber
/// is an [`ExecutionEventSubscriber`] or a [`GeneratorEventSubscriber`].
///
/// [`diagnostic!`]: crate::publish::diagnostic
/// [`ExecutionEventSubscriber`]: crate::ExecutionEventSubscriber
/// [`GeneratorEventSubscriber`]: crate::GeneratorEventSubscriber
pub trait DiagnosticSubscriber: Send + Sync {
    /// Return `false` to opt out of all diagnostic events.
    ///
    /// When `false`, the [`diagnostic!`] macro skips constructing the event
    /// entirely, avoiding string allocations for subscribers that discard
    /// diagnostics (e.g. [`NoopSubscriber`]).
    ///
    /// Defaults to `true`.
    ///
    /// [`diagnostic!`]: crate::publish::diagnostic
    /// [`NoopSubscriber`]: crate::NoopSubscriber
    fn wants_diagnostics(&self) -> bool {
        true
    }

    async fn on_diagnostic(&self, _event: DiagnosticEvent) {}
}

/// Forward all diagnostic calls through a shared reference.
impl<S: DiagnosticSubscriber> DiagnosticSubscriber for &S {
    fn wants_diagnostics(&self) -> bool {
        S::wants_diagnostics(*self)
    }
    async fn on_diagnostic(&self, event: DiagnosticEvent) {
        S::on_diagnostic(*self, event).await
    }
}
