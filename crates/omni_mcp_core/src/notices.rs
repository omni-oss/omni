//! Per-request notice collection for MCP (RFC 0023, 4.5).
//!
//! Each notice is processed in a fixed order: redacted (by `publish_event`,
//! before it reaches the collector), deduplicated, then bounded.

use std::collections::{BTreeMap, HashSet};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use omni_messages::NoticeEvent;
use omni_notice::Notice;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

omni_codes::prefixes!("notices");

/// Maximum number of non-essential notices returned per request.
pub const MAX_NOTICES: usize = 50;
/// Maximum number of omitted notice keys tracked per request.
pub const MAX_TRACKED_DROPPED: usize = 1_000;

/// Some notices were omitted from this response.
///
/// A request produced more distinct notices than a response carries, so the
/// rest were dropped. `dropped` counts unique notices omitted after
/// deduplication; when `dropped_exact` is false it is a lower bound.
#[derive(Notice)]
#[notice(
    code = "notices.truncated",
    level = info,
    stable,
    message = "{at_least}{dropped} further notices were omitted"
)]
pub struct NoticesTruncated {
    pub dropped: usize,
    pub dropped_exact: bool,
    #[notice(skip)]
    pub at_least: &'static str,
}

/// A notice as returned to MCP clients. `target` and `essential` are not sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NoticePayload {
    /// `info`, `warn` or `error`.
    pub level: String,
    /// Stable code; agents match on this. See the `code_explain` tool.
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

impl From<&NoticeEvent> for NoticePayload {
    fn from(e: &NoticeEvent) -> Self {
        Self {
            level: e.level().as_str().to_string(),
            code: e.code().to_string(),
            message: e.message().to_string(),
            fields: e.fields().clone(),
        }
    }
}

#[derive(Default)]
pub struct NoticeCollector {
    retained: Vec<NoticeEvent>,
    seen: HashSet<(String, String, String)>,
    essential_seen: HashSet<&'static str>,
    non_essential: usize,
    dropped: HashSet<u64>,
    overflowed: bool,
}

impl NoticeCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retains a distinct notice for the response. Essential notices are
    /// deduplicated by code and exempt from the non-essential limit. Other
    /// notices are deduplicated by code, message, and canonical fields;
    /// notices beyond [`MAX_NOTICES`] are tracked for the truncation entry.
    pub fn push(&mut self, e: NoticeEvent) {
        if e.essential() {
            // Essential notices are static: the code alone identifies them.
            if self.essential_seen.insert(e.code()) {
                self.retained.push(e);
            }
            return;
        }
        let key = (
            e.code().to_string(),
            e.message().to_string(),
            canonical_fields(e.fields()),
        );
        if self.seen.contains(&key) {
            return;
        }
        if self.non_essential < MAX_NOTICES {
            self.non_essential += 1;
            self.seen.insert(key);
            self.retained.push(e);
            return;
        }
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        let h = h.finish();
        if self.dropped.contains(&h) {
            return;
        }
        if self.dropped.len() < MAX_TRACKED_DROPPED {
            self.dropped.insert(h);
        } else {
            self.overflowed = true;
        }
    }

    /// Returns the notices for the response, appending the truncation entry
    /// last when at least one unique notice was omitted. Its count becomes a
    /// lower bound once tracking exceeds [`MAX_TRACKED_DROPPED`].
    pub fn finish(mut self) -> Vec<NoticePayload> {
        if !self.dropped.is_empty() || self.overflowed {
            let exact = !self.overflowed;
            let n = NoticesTruncated {
                dropped: self.dropped.len(),
                dropped_exact: exact,
                at_least: if exact { "" } else { "at least " },
            };
            self.retained
                .push(NoticeEvent::from_notice(&n, module_path!()));
        }
        self.retained.iter().map(NoticePayload::from).collect()
    }
}

/// Serializes fields compactly, sorting object keys recursively while preserving
/// array order. Returns an empty string if serialization fails.
pub fn canonical_fields(fields: &BTreeMap<String, Value>) -> String {
    fn canon(v: &Value) -> Value {
        match v {
            Value::Object(o) => {
                let sorted: BTreeMap<_, _> =
                    o.iter().map(|(k, v)| (k.clone(), canon(v))).collect();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
            other => other.clone(),
        }
    }
    let obj: serde_json::Map<String, Value> =
        fields.iter().map(|(k, v)| (k.clone(), canon(v))).collect();
    serde_json::to_string(&Value::Object(obj)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    omni_codes::prefixes!("mcptest");

    /// Sample.
    #[derive(Notice)]
    #[notice(code = "mcptest.n", level = warn, message = "m {n}")]
    struct N {
        n: String,
        extra: serde_json::Value,
    }

    /// Essential sample.
    #[derive(Notice)]
    #[notice(code = "mcptest.e", level = warn, essential, message = "e")]
    struct E;

    fn n(n: &str) -> NoticeEvent {
        NoticeEvent::from_notice(
            &N {
                n: n.into(),
                extra: Value::Null,
            },
            "t",
        )
    }
    fn nx(n: &str, extra: Value) -> NoticeEvent {
        NoticeEvent::from_notice(&N { n: n.into(), extra }, "t")
    }
    fn e() -> NoticeEvent {
        NoticeEvent::from_notice(&E, "t")
    }

    fn codes(v: &[NoticePayload]) -> Vec<&str> {
        v.iter().map(|p| p.code.as_str()).collect()
    }

    #[test]
    fn duplicates_collapse() {
        let mut c = NoticeCollector::new();
        c.push(n("a"));
        c.push(n("a"));
        assert_eq!(c.finish().len(), 1);
    }

    #[test]
    fn same_message_different_fields_stay_separate() {
        let mut c = NoticeCollector::new();
        c.push(nx("a", json!(1)));
        c.push(nx("a", json!(2)));
        assert_eq!(c.finish().len(), 2);
    }

    #[test]
    fn nested_key_order_collapses_but_array_order_does_not() {
        let mut c = NoticeCollector::new();
        c.push(nx("a", json!({"x": 1, "y": {"p": 1, "q": 2}})));
        c.push(nx("a", json!({"y": {"q": 2, "p": 1}, "x": 1})));
        assert_eq!(c.finish().len(), 1);

        let mut c = NoticeCollector::new();
        c.push(nx("a", json!([1, 2])));
        c.push(nx("a", json!([2, 1])));
        assert_eq!(c.finish().len(), 2);
    }

    #[test]
    fn exactly_the_cap_adds_no_truncation_entry() {
        let mut c = NoticeCollector::new();
        for i in 0..MAX_NOTICES {
            c.push(n(&i.to_string()));
        }
        let out = c.finish();
        assert_eq!(out.len(), MAX_NOTICES);
        assert!(!codes(&out).contains(&"notices.truncated"));
    }

    #[test]
    fn overflow_appends_one_truncation_entry_last() {
        let mut c = NoticeCollector::new();
        for i in 0..(MAX_NOTICES + 12) {
            c.push(n(&i.to_string()));
        }
        let out = c.finish();
        assert_eq!(out.len(), MAX_NOTICES + 1);
        let last = out.last().unwrap();
        assert_eq!(last.code, "notices.truncated");
        assert_eq!(last.level, "info");
        assert_eq!(last.message, "12 further notices were omitted");
        assert_eq!(last.fields["dropped"], 12);
        assert_eq!(last.fields["dropped_exact"], true);
        assert!(!last.fields.contains_key("at_least"));
    }

    #[test]
    fn repeats_of_omitted_or_retained_notices_do_not_inflate_dropped() {
        let mut c = NoticeCollector::new();
        for i in 0..MAX_NOTICES {
            c.push(n(&i.to_string()));
        }
        for _ in 0..5 {
            c.push(n("0")); // already retained
            c.push(n("over")); // omitted, repeated
        }
        let out = c.finish();
        assert_eq!(out.last().unwrap().fields["dropped"], 1);
    }

    #[test]
    fn essential_is_exempt_from_the_bound_and_deduped_by_code() {
        let mut c = NoticeCollector::new();
        for i in 0..(MAX_NOTICES + 3) {
            c.push(n(&i.to_string()));
        }
        c.push(e());
        c.push(e());
        let out = c.finish();
        assert_eq!(
            codes(&out).iter().filter(|c| **c == "mcptest.e").count(),
            1
        );
        // The late essential notice precedes the truncation entry.
        assert_eq!(out.last().unwrap().code, "notices.truncated");
        assert_eq!(out[out.len() - 2].code, "mcptest.e");
    }

    #[test]
    fn tracking_is_bounded_and_reports_a_lower_bound() {
        let mut c = NoticeCollector::new();
        for i in 0..(MAX_NOTICES + MAX_TRACKED_DROPPED + 10) {
            c.push(n(&i.to_string()));
        }
        let out = c.finish();
        let last = out.last().unwrap();
        assert_eq!(last.fields["dropped_exact"], false);
        assert_eq!(last.fields["dropped"], MAX_TRACKED_DROPPED);
        assert_eq!(
            last.message,
            format!(
                "at least {MAX_TRACKED_DROPPED} further notices were omitted"
            )
        );
    }

    #[test]
    fn payload_wire_format() {
        let p = NoticePayload::from(&e());
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["level"], "warn");
        assert!(v.get("target").is_none());
        assert!(v.get("essential").is_none());
    }
}
