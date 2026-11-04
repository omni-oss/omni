//! The application-wide check of the code registry (RFC 0023): every crate
//! that registers codes is linked into `omni_cli_core`, so this sees them all.

use omni_cli_core as _;
use omni_codes::{CodeKind, NoticeLevel};

#[test]
fn registry_is_valid() {
    if let Err(errors) = omni_codes::validate() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("invalid code registry:\n{}", msgs.join("\n"));
    }
}

#[test]
fn surviving_notice_sites_are_registered() {
    for (code, level, essential) in [
        ("redact.disabled", NoticeLevel::Warn, true),
        ("sandbox.os_disabled", NoticeLevel::Warn, true),
        ("capabilities.not_enforced", NoticeLevel::Warn, false),
        ("capabilities.no_floor", NoticeLevel::Warn, false),
        ("capabilities.ignored", NoticeLevel::Warn, false),
        ("capabilities.import_scan", NoticeLevel::Warn, false),
        ("runtime.experimental", NoticeLevel::Warn, false),
        ("meta.filter_excluded", NoticeLevel::Error, false),
    ] {
        let info = omni_codes::get(code)
            .unwrap_or_else(|| panic!("`{code}` is not registered"));
        assert_eq!(info.kind(), CodeKind::Notice, "{code}");
        assert_eq!(info.level(), Some(level), "{code}");
        assert_eq!(info.essential(), essential, "{code}");
    }
}

#[test]
fn error_and_notice_pairs_are_related() {
    let failed = omni_codes::get("meta.filter_failed").unwrap();
    assert_eq!(failed.kind(), CodeKind::Error);
    assert!(
        omni_codes::get("meta.filter_excluded")
            .unwrap()
            .related
            .contains(&"meta.filter_failed")
    );
}

#[test]
fn essential_codes_are_static() {
    // The derive rejects fields and placeholders on essential notices, so an
    // essential code's emissions are identical (MCP deduplicates by code).
    let essential: Vec<_> =
        omni_codes::all().filter(|c| c.essential()).map(|c| c.code).collect();
    assert!(essential.contains(&"redact.disabled"));
    assert!(essential.contains(&"sandbox.os_disabled"));
}
