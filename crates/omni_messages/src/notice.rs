use std::collections::BTreeMap;

use omni_diagnostic::Diagnostic;
use omni_notice::Notice;
use serde::Serialize;

pub use omni_codes::NoticeLevel;

/// The plain data of a published [`Notice`], as delivered to subscribers.
///
/// Fields are private and [`NoticeEvent::from_notice`] is the only
/// constructor, so an event whose `(code, level, essential)` disagrees with
/// its notice type cannot be built.
#[derive(Debug, Clone, Serialize)]
pub struct NoticeEvent {
    level: NoticeLevel,
    code: &'static str,
    essential: bool,
    message: String,
    fields: BTreeMap<String, serde_json::Value>,
    /// Emitting module, for the `tracing` mirror. Not sent to MCP.
    #[serde(skip)]
    target: &'static str,
    /// A diagnostic's rendered body (`diagnostic_notice!` only), for CLI
    /// layout. The structured form is in `fields.details`; not sent to MCP.
    #[serde(skip)]
    body: Option<String>,
}

impl NoticeEvent {
    pub fn from_notice<N: Notice>(n: &N, target: &'static str) -> Self {
        Self {
            level: n.level(),
            code: n.code(),
            essential: n.essential(),
            message: n.to_string(),
            fields: n.fields(),
            target,
            body: None,
        }
    }

    /// Like [`from_notice`](Self::from_notice), adding the diagnostic's
    /// `details` and `hints` to `fields`. The code, level and message always
    /// come from the notice, never from the diagnostic.
    pub fn from_diagnostic_notice<N: Notice, D: Diagnostic + ?Sized>(
        n: &N,
        d: &D,
        target: &'static str,
    ) -> Self {
        let mut e = Self::from_notice(n, target);
        if let Some(details) = d.details() {
            e.fields.insert("details".into(), details);
        }
        let mut body = String::new();
        if d.render_body(&mut body).is_ok() && !body.trim().is_empty() {
            e.body = Some(body);
        }
        let hints = d.hints();
        if !hints.is_empty() {
            e.fields.insert("hints".into(), serde_json::json!(hints));
        }
        e
    }

    pub fn level(&self) -> NoticeLevel {
        self.level
    }
    pub fn code(&self) -> &'static str {
        self.code
    }
    pub fn essential(&self) -> bool {
        self.essential
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn fields(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.fields
    }
    pub fn target(&self) -> &'static str {
        self.target
    }
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// Redacts the message and every string in `fields` (RFC 0022).
    pub fn redact(&mut self, redactor: &omni_redact::Redactor) {
        if redactor.is_empty() {
            return;
        }
        self.message = redactor.redact(self.message.as_str()).into_owned();
        if let Some(b) = &mut self.body {
            *b = redactor.redact(b.as_str()).into_owned();
        }
        for v in self.fields.values_mut() {
            redact_value(v, redactor);
        }
    }
}

fn redact_value(v: &mut serde_json::Value, r: &omni_redact::Redactor) {
    match v {
        serde_json::Value::String(s) => *s = r.redact(s.as_str()).into_owned(),
        serde_json::Value::Array(a) => {
            a.iter_mut().for_each(|x| redact_value(x, r))
        }
        serde_json::Value::Object(o) => {
            o.values_mut().for_each(|x| redact_value(x, r))
        }
        _ => {}
    }
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

    /// Redactor applied once per notice before dispatch (RFC 0022).
    fn redactor(&self) -> Option<&omni_redact::Redactor> {
        None
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
    fn redactor(&self) -> Option<&omni_redact::Redactor> {
        S::redactor(*self)
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
