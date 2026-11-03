use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A structured notice message emitted by business logic in place of
/// direct `log::*` / `println!` calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeEvent {
    pub level: NoticeLevel,
    pub message: String,
    pub fields: BTreeMap<String, serde_json::Value>,
    pub target: String,
}

/// Severity of a [`NoticeEvent`].
///
/// Notices are user-facing messages, so only `Info`, `Warn` and `Error` exist.
/// Developer-oriented `debug` / `trace` output belongs in `log::*` /
/// `tracing::*` and is intentionally not representable here.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display,
)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

/// Shared notice channel used by both execution and generator subscribers.
///
/// Extracted as a supertrait so the [`notice!`] macro and call sites
/// can use a single unambiguous method, regardless of whether the subscriber
/// is an [`ExecutionEventSubscriber`] or a [`GeneratorEventSubscriber`].
///
/// [`notice!`]: crate::publish::notice
/// [`ExecutionEventSubscriber`]: crate::ExecutionEventSubscriber
/// [`GeneratorEventSubscriber`]: crate::GeneratorEventSubscriber
pub trait NoticeSubscriber: Send + Sync {
    /// Return `false` to opt out of all notice events.
    ///
    /// When `false`, the [`notice!`] macro skips constructing the event
    /// entirely, avoiding string allocations for subscribers that discard
    /// notices (e.g. [`NoopSubscriber`]).
    ///
    /// Defaults to `true`.
    ///
    /// [`notice!`]: crate::publish::notice
    /// [`NoopSubscriber`]: crate::NoopSubscriber
    fn wants_notices(&self) -> bool {
        true
    }

    fn on_notice(&self, _event: NoticeEvent) {}

    /// Specialized version of [`on_notice`] for batched notice events.
    /// Override to provide a more efficient implementation. By default,
    /// this method simply calls [`on_notice`] for each event.
    fn on_notices_batched(
        &self,
        _events: impl IntoIterator<Item = NoticeEvent>,
    ) {
        for event in _events {
            self.on_notice(event);
        }
    }
}

/// Forward all notice calls through a shared reference.
impl<S: NoticeSubscriber> NoticeSubscriber for &S {
    fn wants_notices(&self) -> bool {
        S::wants_notices(*self)
    }
    fn on_notice(&self, event: NoticeEvent) {
        S::on_notice(*self, event)
    }
    fn on_notices_batched(
        &self,
        events: impl IntoIterator<Item = NoticeEvent>,
    ) {
        S::on_notices_batched(*self, events)
    }
}
