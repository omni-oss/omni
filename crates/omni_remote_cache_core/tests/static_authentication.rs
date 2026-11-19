use std::sync::Arc;

use maps::{UnorderedMap, unordered_map};
use omni_remote_cache_core::{
    config::{
        AllOrSpecificConfiguration, ApiKeyConfiguration, ScopesConfiguration,
        SecurityConfiguration,
    },
    data_impl::in_memory::InMemoryDatabase,
    hooks::{api_key_id, api_key_id_keyed},
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    security::{
        Authenticator, Caller, CredentialKind, InMemoryAuthenticator, Resource,
        Scope, Scopes,
    },
};

const ALL: [Scope; 5] = [
    Scope::Read,
    Scope::Write,
    Scope::List,
    Scope::Delete,
    Scope::Publish,
];

fn key() -> ApiKeyConfiguration {
    ApiKeyConfiguration {
        enabled: true,
        ..Default::default()
    }
}

fn resource() -> Resource {
    Resource {
        tenant: "t".into(),
        org: "o".into(),
        ws: "w".into(),
        env: "e".into(),
    }
}

fn authenticator(
    config: ApiKeyConfiguration,
    secret: Option<&str>,
) -> InMemoryAuthenticator {
    InMemoryAuthenticator::new(
        Arc::new(unordered_map!("key".into() => config)),
        secret.map(str::to_owned),
    )
}

async fn caller(config: ApiKeyConfiguration, secret: Option<&str>) -> Caller {
    authenticator(config, secret)
        .authenticate("key", &resource(), ALL.into())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn legacy_all_grants_each_old_scope_but_never_publish() {
    let caller = caller(key(), None).await;
    for scope in ALL {
        assert_eq!(caller.can(scope), scope != Scope::Publish, "{scope:?}");
    }
    assert_eq!(caller.credential().kind(), &CredentialKind::ApiKey);
    assert_eq!(caller.credential().id(), api_key_id("key"));
    assert_eq!(caller.principal(), caller.credential().id());
    assert_eq!(caller.scope_pin(), None);
}

#[tokio::test]
async fn explicit_grants_are_independent_and_limited_to_wanted() {
    for (configured, allowed) in [
        (ScopesConfiguration::ReadArtifacts, Scope::Read),
        (ScopesConfiguration::WriteArtifacts, Scope::Write),
        (ScopesConfiguration::ListArtifacts, Scope::List),
        (ScopesConfiguration::DeleteArtifacts, Scope::Delete),
        (ScopesConfiguration::PublishArtifacts, Scope::Publish),
    ] {
        let mut config = key();
        config.scopes = AllOrSpecificConfiguration::Specific(
            [configured].into_iter().collect(),
        );
        let auth = authenticator(config.clone(), None);
        let caller = auth
            .authenticate("key", &resource(), ALL.into())
            .await
            .unwrap()
            .unwrap();
        for scope in ALL {
            assert_eq!(
                caller.can(scope),
                scope == allowed,
                "{allowed:?}/{scope:?}"
            );
        }
        let empty = auth
            .authenticate("key", &resource(), Scopes::default())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(empty.scopes(), Scopes::default());
    }
}

#[tokio::test]
async fn unknown_disabled_and_expired_keys_are_not_authenticated() {
    let auth = authenticator(key(), None);
    assert!(
        auth.authenticate("unknown", &resource(), ALL.into())
            .await
            .unwrap()
            .is_none()
    );
    let mut disabled = key();
    disabled.enabled = false;
    let mut expired = key();
    expired.expires_at = Some(time::OffsetDateTime::UNIX_EPOCH);
    for config in [disabled, expired] {
        assert!(
            authenticator(config, None)
                .authenticate("key", &resource(), ALL.into())
                .await
                .unwrap()
                .is_none()
        );
    }
    let mut future = key();
    future.expires_at =
        Some(time::OffsetDateTime::now_utc() + time::Duration::days(365));
    assert!(
        authenticator(future, None)
            .authenticate("key", &resource(), ALL.into())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn every_resource_dimension_is_checked_and_a_denial_remains_recognized() {
    let mut config = key();
    config.tenants = AllOrSpecificConfiguration::Specific(
        ["t".into()].into_iter().collect(),
    );
    config.organizations = AllOrSpecificConfiguration::Specific(
        ["o".into()].into_iter().collect(),
    );
    config.workspaces = AllOrSpecificConfiguration::Specific(
        ["w".into()].into_iter().collect(),
    );
    config.environments = AllOrSpecificConfiguration::Specific(
        ["e".into()].into_iter().collect(),
    );
    let auth = authenticator(config, None);
    let target = resource();
    assert!(
        auth.authenticate("key", &target, Scope::Read.into())
            .await
            .unwrap()
            .unwrap()
            .can(Scope::Read)
    );
    for denied in [
        Resource {
            tenant: "other".into(),
            ..target.clone()
        },
        Resource {
            org: "other".into(),
            ..target.clone()
        },
        Resource {
            ws: "other".into(),
            ..target.clone()
        },
        Resource {
            env: "other".into(),
            ..target.clone()
        },
    ] {
        let caller = auth
            .authenticate("key", &denied, ALL.into())
            .await
            .unwrap()
            .expect("valid key must not fall through on a resource denial");
        assert_eq!(caller.scopes(), Scopes::default(), "{denied:?}");
    }
}

#[tokio::test]
async fn secret_rotation_changes_default_principals_but_not_explicit_ones() {
    let unkeyed = caller(key(), None).await;
    let a = caller(key(), Some("secret-a")).await;
    let b = caller(key(), Some("secret-b")).await;
    assert_eq!(a.credential().id(), api_key_id_keyed("secret-a", "key"));
    assert_eq!(a.principal(), a.credential().id());
    assert_ne!(a.principal(), b.principal());
    assert_ne!(a.principal(), unkeyed.principal());

    let mut config = key();
    config.principal = Some("stable-owner".into());
    for secret in [None, Some("secret-a"), Some("secret-b")] {
        assert_eq!(
            caller(config.clone(), secret).await.principal(),
            "stable-owner"
        );
    }
}

#[tokio::test]
async fn invalid_principal_fails_closed_instead_of_using_a_default() {
    for principal in ["".into(), "x".repeat(257), "bad\nname".into()] {
        let mut config = key();
        config.principal = Some(principal);
        assert!(
            authenticator(config, None)
                .authenticate("key", &resource(), ALL.into())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn provider_applies_the_configured_secret_to_caller_ids() {
    // Construct data through the config API, as the service does.
    let data: InMemoryDatabase =
        omni_remote_cache_core::config::Configuration {
            tenants: UnorderedMap::default(),
            security: SecurityConfiguration {
                api_keys: UnorderedMap::default(),
                api_key_id_secret: None,
            },
        }
        .to_in_memory_database();
    let provider = ConfigBasedDependencyProvider::new(
        Arc::new(data),
        Arc::new(unordered_map!("key".into() => key())),
    )
    .with_api_key_id_secret(Some("secret".into()));
    let caller = provider
        .authenticator()
        .authenticate("key", &resource(), Scope::Read.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(caller.credential().id(), api_key_id_keyed("secret", "key"));
}

#[test]
fn old_json_remains_valid_and_new_identity_fields_round_trip() {
    let old: SecurityConfiguration =
        serde_json::from_value(serde_json::json!({
            "api_keys": { "key": { "scopes": "all" } }
        }))
        .unwrap();
    assert!(old.api_key_id_secret.is_none());
    assert!(old.api_keys["key"].principal.is_none());
    assert!(old.api_keys["key"].enabled);

    let new: SecurityConfiguration =
        serde_json::from_value(serde_json::json!({
            "api_key_id_secret": "secret",
            "api_keys": { "key": {
                "principal": "owner",
                "scopes": ["publish:artifacts"]
            } }
        }))
        .unwrap();
    let round_trip: SecurityConfiguration =
        serde_json::from_value(serde_json::to_value(&new).unwrap()).unwrap();
    assert_eq!(round_trip, new);
    assert_eq!(new.api_keys["key"].principal.as_deref(), Some("owner"));
    assert_eq!(new.api_key_id_secret.as_deref(), Some("secret"));
}

#[test]
fn invalid_static_identity_settings_fail_at_config_load_without_exposing_values()
 {
    use omni_remote_cache_core::config::Configuration;
    for principal in ["".into(), "x".repeat(257), "bad\nname".into()] {
        let json = serde_json::json!({
            "tenants": {},
            "security": { "api_keys": { "private-credential": {
                "principal": principal
            } } }
        });
        let error = Configuration::from_inline(json.to_string()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid principal in static API-key configuration"
        );
        assert!(!format!("{error:?}").contains("private-credential"));
    }
    let error = Configuration::from_inline(
        serde_json::json!({
            "tenants": {},
            "security": { "api_keys": {}, "api_key_id_secret": "" }
        })
        .to_string(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "static api_key_id_secret must not be empty"
    );
}

#[tokio::test]
async fn direct_authenticator_use_also_rejects_an_empty_secret() {
    assert!(
        authenticator(key(), Some(""))
            .authenticate("key", &resource(), ALL.into())
            .await
            .is_err()
    );
}
