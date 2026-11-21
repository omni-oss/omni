use std::path::PathBuf;

use eyre::WrapErr;
use omni_diagnostic::{Audience, DiagnosticReport, find};
use system_traits::impls::InMemorySys;

use crate::{
    GetRemoteCachingConfigError, SetupRemoteCachingConfigError,
    crypto::{CryptoError, CryptoErrorInner},
    get_remote_caching_config::GetRemoteCachingConfigErrorInner,
    secret_key::{SecretKeyError, SecretKeyErrorInner},
    setup_remote_caching_config::SetupRemoteCachingConfigErrorInner,
};

fn report(err: SetupRemoteCachingConfigError) -> eyre::Report {
    Err::<(), _>(err)
        .wrap_err("setting up remote caching")
        .unwrap_err()
}

#[test]
fn invalid_access_is_a_user_diagnostic_without_the_server_message() {
    let r = report(SetupRemoteCachingConfigError::from(
        SetupRemoteCachingConfigErrorInner::InvalidAccess(eyre::eyre!(
            "tenant acme-secret-token was rejected"
        )),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "remote_cache.invalid_access");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details(), None);
    assert_eq!(d.hints().len(), 1);
    assert!(!d.hints().join(" ").contains("acme-secret-token"));
}

#[test]
fn insufficient_scopes_is_a_user_diagnostic_that_names_what_the_key_holds() {
    let r = report(SetupRemoteCachingConfigError::from(
        SetupRemoteCachingConfigErrorInner::InsufficientScopes {
            scopes: "publish:artifacts".into(),
        },
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "remote_cache.insufficient_scopes");
    assert_eq!(d.audience(), Audience::User);
    assert_eq!(d.details().unwrap()["scopes"], "publish:artifacts");
    assert!(d.hints()[0].contains("read:artifacts"));
}

#[test]
fn unreadable_config_names_the_path_and_is_environment() {
    let sys = InMemorySys::default();
    let path = PathBuf::from("/cache/remote.enc");
    let err = crate::get_remote_caching_config("user", &path, true, &sys)
        .unwrap_err();
    let r: eyre::Report = err.into();

    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "remote_cache.config_unreadable");
    assert_eq!(d.audience(), Audience::Environment);
    assert_eq!(d.details().unwrap()["path"], "/cache/remote.enc");
    assert!(d.hints()[0].contains("/cache/remote.enc"));
}

#[test]
fn unreadable_config_is_found_through_wrap_err_and_delegation() {
    let config: eyre::Report = GetRemoteCachingConfigError::from(
        GetRemoteCachingConfigErrorInner::ConfigUnreadable {
            path: PathBuf::from("remote.enc"),
            source: eyre::eyre!("ciphertext too short"),
        },
    )
    .into();
    let r = report(SetupRemoteCachingConfigError::from(
        SetupRemoteCachingConfigErrorInner::Custom(config),
    ));
    let d = find(&r).expect("diagnostic");
    assert_eq!(d.code(), "remote_cache.config_unreadable");
    assert_eq!(d.details().unwrap()["path"], "remote.enc");
}

#[test]
fn plumbing_failures_stay_uncoded() {
    let crypto: eyre::Report = CryptoError::from(
        CryptoErrorInner::InvalidKeyLength(3, eyre::eyre!("bad key")),
    )
    .into();
    assert!(find(&crypto).is_none());

    let io = report(SetupRemoteCachingConfigError::from(
        SetupRemoteCachingConfigErrorInner::Io(std::io::Error::other("x")),
    ));
    assert!(find(&io).is_none());

    let custom = report(SetupRemoteCachingConfigError::from(
        SetupRemoteCachingConfigErrorInner::Custom(eyre::eyre!("boom")),
    ));
    assert!(find(&custom).is_none());
}

#[test]
fn keyring_diagnostic_survives_setup_without_backend_data() {
    let sentinel = "credential-backend-sentinel";
    let key_error = SecretKeyError::from(keyring_core::Error::NoStorageAccess(
        Box::new(std::io::Error::other(sentinel)),
    ));
    let report = report(SetupRemoteCachingConfigError::from(key_error));
    let rendered = DiagnosticReport::from_report(&report).unwrap();
    assert_eq!(rendered.exit_code(), 1);
    assert_eq!(
        rendered.to_json()["kind"],
        "remote_cache.keyring_unavailable"
    );
    for output in [
        format!("{report:?}"),
        rendered.to_text(),
        rendered.to_json().to_string(),
    ] {
        assert!(!output.contains(sentinel));
    }
    assert!(
        report
            .chain()
            .all(|cause| !cause.to_string().contains(sentinel))
    );
}

#[test]
fn machine_identifier_diagnostic_survives_config_loading() {
    let key_error = SecretKeyError::from(SecretKeyErrorInner::MachineId);
    let report =
        eyre::Report::new(GetRemoteCachingConfigError::from(key_error));
    let diagnostic = find(&report).unwrap();
    assert_eq!(diagnostic.code(), "remote_cache.machine_id_unavailable");
    assert_eq!(diagnostic.audience(), Audience::Environment);
}

#[test]
fn codes_are_registered_in_the_catalog() {
    for code in [
        "remote_cache.invalid_access",
        "remote_cache.insufficient_scopes",
        "remote_cache.config_unreadable",
        "remote_cache.keyring_unavailable",
        "remote_cache.machine_id_unavailable",
    ] {
        assert!(
            omni_codes::get(code).is_some(),
            "`{code}` is not registered"
        );
    }
    assert!(omni_codes::validate().is_ok());
}
