//! The `Notice` trait (RFC 0023).
//!
//! A notice is a message about *this run* that the user may want to know even
//! though the operation completed. It is a derived type with a mandatory
//! stable code, a fixed level, a message and typed fields.
//!
//! `Notice` is sealed (through [`omni_codes::Code`]): it can only be
//! implemented with `#[derive(Notice)]`.

extern crate self as omni_notice;

use std::{collections::BTreeMap, fmt};

pub use omni_codes::{Code, CodeDetail, CodeInfo, NoticeLevel};
pub use omni_notice_derive::Notice;

pub trait Notice: Code + fmt::Display + Send + Sync + 'static {
    fn code(&self) -> &'static str {
        self.info().code
    }

    fn level(&self) -> NoticeLevel {
        match self.info().detail {
            CodeDetail::Notice { level, .. } => level,
            CodeDetail::Error { .. } => {
                unreachable!("a notice is never an error code")
            }
        }
    }

    fn essential(&self) -> bool {
        self.info().essential()
    }

    /// Typed fields as JSON, keyed by the struct's field names.
    fn fields(&self) -> BTreeMap<String, serde_json::Value>;
}

#[doc(hidden)]
pub mod __private {
    pub use omni_codes;
    pub use serde_json;

    pub fn to_value<T: serde::Serialize + ?Sized>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    omni_codes::prefixes!("sample");

    /// Sample notice.
    ///
    /// Longer explanation.
    #[derive(Notice)]
    #[notice(code = "sample.fields", level = error, stable,
             message = "could not write '{entry}' ({count})",
             related = "sample.static_one")]
    struct WithFields {
        entry: String,
        count: u32,
    }

    /// Static essential notice.
    #[derive(Notice)]
    #[notice(code = "sample.static_one", level = warn, essential,
             message = "static {{braces}}")]
    struct StaticOne;

    #[test]
    fn display_fields_and_info() {
        let n = WithFields {
            entry: "a#b".into(),
            count: 3,
        };
        assert_eq!(n.to_string(), "could not write 'a#b' (3)");
        assert_eq!(n.code(), "sample.fields");
        assert_eq!(n.level(), NoticeLevel::Error);
        assert!(!n.essential());
        let f = n.fields();
        assert_eq!(f["entry"], "a#b");
        assert_eq!(f["count"], 3);
        assert_eq!(n.info().summary, "Sample notice.");
        assert_eq!(n.info().explanation, "Longer explanation.");
        assert!(n.info().stable);
    }

    #[test]
    fn static_notice() {
        assert_eq!(StaticOne.to_string(), "static {braces}");
        assert!(StaticOne.essential());
        assert!(StaticOne.fields().is_empty());
    }

    #[cfg(not(feature = "explicit-registry"))]
    #[test]
    fn registered_and_valid() {
        assert!(omni_codes::get("sample.fields").is_some());
        assert_eq!(
            omni_codes::get("sample.static_one").unwrap().level(),
            Some(NoticeLevel::Warn)
        );
        omni_codes::validate().unwrap();
    }
}
