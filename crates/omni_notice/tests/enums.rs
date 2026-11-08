use omni_notice::{Code, Notice, NoticeLevel};

omni_codes::prefixes!("enum_test");

const EXPLANATION: &str = "Explicit explanation.";

// A message-only value deliberately does not implement Serialize.
struct Reason;

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unavailable")
    }
}

#[derive(Notice)]
enum CacheNotice {
    /// Could not write an entry.
    ///
    /// This run still has the result.
    #[notice(code = "enum_test.write", level = error, stable,
             related = "enum_test.disabled",
             message = "entry {entry:?}: {reason}")]
    Write {
        entry: String,
        count: u32,
        #[notice(skip)]
        reason: Reason,
    },
    /// Verification is disabled.
    #[notice(code = "enum_test.disabled", level = warn, essential,
             message = "disabled {{verification}}")]
    Disabled,
    /// An old informational notice.
    #[notice(code = "enum_test.old", level = info, stable, deprecated,
             docs = EXPLANATION, message = "old")]
    Old {},
    /// Empty essential named variant.
    #[notice(code = "enum_test.empty_fields", level = info, essential,
             message = "empty")]
    Empty {},
    /// Fields cannot shadow generated bindings.
    #[notice(code = "enum_test.bindings", level = info,
             message = "{__f} {__m} {__notice_field_0}")]
    Bindings {
        __f: String,
        __m: String,
        __notice_field_0: String,
    },
}

#[derive(Notice)]
enum Empty {}

/// Structs share the case generation and can use implementation-like names.
#[derive(Notice)]
#[notice(code = "enum_test.struct", level = info, message = "{__m}")]
struct StructNotice {
    __m: String,
}

#[test]
fn active_variant_controls_display_fields_and_metadata() {
    let notice = CacheNotice::Write {
        entry: "task".into(),
        count: 3,
        reason: Reason,
    };
    assert_eq!(notice.to_string(), "entry \"task\": unavailable");
    assert_eq!(notice.code(), "enum_test.write");
    assert_eq!(notice.level(), NoticeLevel::Error);
    assert!(!notice.essential());
    assert_eq!(
        notice.fields(),
        std::collections::BTreeMap::from([
            ("entry".into(), serde_json::json!("task")),
            ("count".into(), serde_json::json!(3)),
        ])
    );
    assert_eq!(notice.info().summary, "Could not write an entry.");
    assert_eq!(notice.info().explanation, "This run still has the result.");
    assert_eq!(notice.info().related, &["enum_test.disabled"]);
    assert!(notice.info().stable);
    assert!(!notice.info().deprecated);

    let disabled = CacheNotice::Disabled;
    assert_eq!(disabled.to_string(), "disabled {verification}");
    assert_eq!(disabled.level(), NoticeLevel::Warn);
    assert!(disabled.essential());
    assert!(disabled.fields().is_empty());
    assert!(!disabled.info().stable);

    let old = CacheNotice::Old {};
    assert_eq!(old.level(), NoticeLevel::Info);
    assert_eq!(old.to_string(), "old");
    assert!(old.fields().is_empty());
    assert!(old.info().deprecated);
    assert!(old.info().stable);
    assert_eq!(old.info().explanation, EXPLANATION);
    assert!(CacheNotice::Empty {}.essential());
    assert!(Empty::CODES.is_empty());
}

#[test]
fn generated_identifiers_do_not_collide() {
    let notice = CacheNotice::Bindings {
        __f: "formatter".into(),
        __m: "map".into(),
        __notice_field_0: "binding".into(),
    };
    assert_eq!(notice.to_string(), "formatter map binding");
    assert_eq!(notice.fields()["__notice_field_0"], "binding");
    assert_eq!(
        StructNotice {
            __m: "struct".into()
        }
        .to_string(),
        "struct"
    );
}

/// Checks declaration-order metadata and independent inventory entries through
/// the default backend or one explicitly installed enum registration.
#[test]
fn one_registration_contains_separate_inventory_entries() {
    let registration = &CacheNotice::__CODE_REGISTRATION;
    assert_eq!(registration.codes, CacheNotice::CODES);
    assert!(registration.prefixes.is_empty());
    assert_eq!(
        registration
            .codes
            .iter()
            .map(|c| c.code)
            .collect::<Vec<_>>(),
        [
            "enum_test.write",
            "enum_test.disabled",
            "enum_test.old",
            "enum_test.empty_fields",
            "enum_test.bindings",
        ]
    );

    #[cfg(feature = "explicit-registry")]
    {
        static PREFIX: omni_codes::CodeRegistration =
            omni_codes::CodeRegistration {
                crate_name: env!("CARGO_PKG_NAME"),
                codes: &[],
                prefixes: &["enum_test"],
            };
        static REGISTRATIONS: &[&omni_codes::CodeRegistration] = &[
            &PREFIX,
            &CacheNotice::__CODE_REGISTRATION,
            &StructNotice::__CODE_REGISTRATION,
            &Empty::__CODE_REGISTRATION,
        ];
        assert!(omni_codes::install(REGISTRATIONS));
    }

    for info in CacheNotice::CODES {
        assert_eq!(omni_codes::get(info.code), Some(info));
    }
    assert_eq!(
        omni_codes::registrations()
            .filter(|r| r.codes.iter().any(|c| c.code == "enum_test.write"))
            .count(),
        1
    );
    assert_eq!(omni_codes::catalog::entries().len(), 6);
    omni_codes::validate().unwrap();
}
