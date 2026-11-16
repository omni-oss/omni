use std::path::{Path, PathBuf};

use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsRead, FsReadAsync};

use crate::{LoadConfigError, utils};

/// How strictly shared remote entries are verified (RFC 0030, section 4.5).
/// Ordered from the loosest to the strictest, so a local setting can only
/// tighten what the workspace chose.
#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMode {
    /// `require` when the workspace trusts any key, otherwise `off`.
    #[default]
    Auto,
    /// No verification.
    Off,
    /// Verify and report failures, but restore the entry anyway.
    Warn,
    /// A failure is a miss.
    Require,
}

/// A public key the workspace trusts to sign shared cache entries.
#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrustedKeyConfiguration {
    /// The first 16 hexadecimal characters of the SHA-256 of the public key.
    /// Checked against `public_key`: a different key needs a different id.
    pub id: String,
    /// An optional label such as `ci-2026-11`.
    #[serde(default)]
    pub name: Option<String>,
    /// The Ed25519 public key, 32 bytes of standard base64.
    pub public_key: String,
}

/// Signature policy committed with the workspace. Machine-local settings may
/// only tighten it.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Default,
)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSigningConfiguration {
    #[serde(default)]
    pub trusted_keys: Vec<TrustedKeyConfiguration>,
    #[serde(default)]
    pub verify: VerifyMode,
}

/// The `remote_cache` section of `workspace.omni.yaml`.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Default,
)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRemoteCacheConfiguration {
    #[serde(default)]
    pub signing: WorkspaceSigningConfiguration,
}

/// Where a publisher's private key is read from. Never the key itself, so it
/// cannot end up in a config file.
#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SigningKeyConfiguration {
    /// The id of the key (see `omni cache keygen`).
    pub id: String,
    /// The environment variable holding the base64 private key.
    #[serde(default)]
    pub private_key_env: Option<String>,
    /// A file holding the base64 private key.
    #[serde(default)]
    pub private_key_file: Option<PathBuf>,
}

/// Where an upload goes (RFC 0030). `shared` needs `publish:artifacts`.
#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Default,
)]
#[serde(rename_all = "lowercase")]
pub enum RemoteTarget {
    #[default]
    Isolated,
    Shared,
}

/// How the isolated scope label is chosen: `auto` derives it from the CI
/// environment, `run` uses the CI run id, anything else is a literal label.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ScopeConfig {
    #[default]
    Auto,
    Run,
    Literal(String),
}

impl ScopeConfig {
    /// A literal must be a label the server accepts, and cannot be one of the
    /// reserved words.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "auto" => Ok(Self::Auto),
            "run" => Ok(Self::Run),
            // Almost certainly a typo for a reserved word, not a label.
            other
                if other.eq_ignore_ascii_case("auto")
                    || other.eq_ignore_ascii_case("run") =>
            {
                Err(format!(
                    "invalid scope {other:?}: the reserved words are lowercase `auto` and `run`"
                ))
            }
            literal => {
                let valid = literal
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphanumeric())
                    && literal.len() <= 64
                    && literal.bytes().all(|b| {
                        b.is_ascii_alphanumeric()
                            || matches!(b, b'.' | b'_' | b'-')
                    });
                if valid {
                    Ok(Self::Literal(literal.to_owned()))
                } else {
                    Err(format!(
                        "invalid scope {literal:?}: use `auto`, `run`, or 1 to 64 letters, digits, '.', '_' or '-' starting with a letter or digit"
                    ))
                }
            }
        }
    }

    pub fn as_text(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Run => "run",
            Self::Literal(label) => label,
        }
    }
}

impl Serialize for ScopeConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_text())
    }
}

impl<'de> Deserialize<'de> for ScopeConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ScopeConfig {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ScopeConfig".into()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        String::json_schema(generator)
    }
}

/// The upload and scope settings of the machine-local remote cache file.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RemoteCacheSettings {
    pub push: bool,
    pub target: RemoteTarget,
    pub scope: ScopeConfig,
    pub max_artifact_size: Option<u64>,
    pub signing_key: Option<SigningKeyConfiguration>,
    pub verify: Option<VerifyMode>,
    pub trusted_keys: Option<Vec<String>>,
}

// This file is stored as MessagePack, which writes a struct as a positional
// array. New fields must only be appended at the end, each with
// `serde(default)`, and none may use `skip_serializing_if`, or files written
// by older versions would decode into the wrong fields.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Merge, Validate,
)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct RemoteCacheConfiguration {
    #[merge(strategy = config_utils::replace)]
    pub api_key: String,

    #[merge(strategy = config_utils::replace)]
    pub api_base_url: String,

    #[merge(strategy = config_utils::replace)]
    pub tenant_code: String,

    #[merge(strategy = config_utils::replace)]
    pub organization_code: String,

    #[merge(strategy = config_utils::replace)]
    pub workspace_code: String,

    #[merge(strategy = config_utils::replace_if_some)]
    pub environment_code: Option<String>,

    /// Upload successful results on a miss. Off by default: machines only read
    /// unless they are told to publish.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub push: bool,

    /// Where uploads go.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub target: RemoteTarget,

    /// The isolated scope label: `auto`, `run` or a literal.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub scope: ScopeConfig,

    /// The largest artifact to upload or download, in bytes. The server's own
    /// limit can only lower it.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub max_artifact_size: Option<u64>,

    /// The publisher's signing key (a reference, never the key).
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub signing_key: Option<SigningKeyConfiguration>,

    /// A local verify mode. It can only tighten the workspace's.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub verify: Option<VerifyMode>,

    /// Narrows the workspace's trusted keys to these ids. `None` leaves them
    /// as they are; `Some([])` trusts none.
    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub trusted_keys: Option<Vec<String>>,
}

impl RemoteCacheConfiguration {
    pub fn settings(&self) -> RemoteCacheSettings {
        RemoteCacheSettings {
            push: self.push,
            target: self.target,
            scope: self.scope.clone(),
            max_artifact_size: self.max_artifact_size,
            signing_key: self.signing_key.clone(),
            verify: self.verify,
            trusted_keys: self.trusted_keys.clone(),
        }
    }

    #[allow(clippy::result_large_err)]
    pub async fn load_async<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
    }

    #[allow(clippy::result_large_err)]
    pub fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsRead + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config(path, sys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape written before `push` and friends existed.
    #[derive(Serialize)]
    struct V0 {
        api_key: &'static str,
        api_base_url: &'static str,
        tenant_code: &'static str,
        organization_code: &'static str,
        workspace_code: &'static str,
        environment_code: Option<&'static str>,
    }

    const V0_VALUE: V0 = V0 {
        api_key: "k",
        api_base_url: "http://h",
        tenant_code: "t",
        organization_code: "o",
        workspace_code: "w",
        environment_code: Some("e"),
    };

    #[test]
    fn a_file_written_before_the_trust_settings_loads_with_defaults() {
        let bytes = rmp_serde::to_vec(&V0_VALUE).unwrap();
        let config: RemoteCacheConfiguration =
            rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(config.api_key, "k");
        assert_eq!(config.environment_code.as_deref(), Some("e"));
        assert_eq!(config.settings(), RemoteCacheSettings::default());
        assert!(!config.push);
        assert_eq!(config.target, RemoteTarget::Isolated);
        assert_eq!(config.scope, ScopeConfig::Auto);

        let yaml = r#"{"api_key":"k","api_base_url":"http://h","tenant_code":"t","organization_code":"o","workspace_code":"w"}"#;
        let config: RemoteCacheConfiguration =
            serde_json::from_str(yaml).unwrap();
        assert_eq!(config.settings(), RemoteCacheSettings::default());
    }

    /// The shape written by the slice that added `push` and friends.
    #[derive(Serialize)]
    struct V1 {
        api_key: &'static str,
        api_base_url: &'static str,
        tenant_code: &'static str,
        organization_code: &'static str,
        workspace_code: &'static str,
        environment_code: Option<&'static str>,
        push: bool,
        target: RemoteTarget,
        scope: ScopeConfig,
        max_artifact_size: Option<u64>,
    }

    #[test]
    fn a_file_written_before_signing_loads_with_no_signing_settings() {
        let bytes = rmp_serde::to_vec(&V1 {
            api_key: "k",
            api_base_url: "http://h",
            tenant_code: "t",
            organization_code: "o",
            workspace_code: "w",
            environment_code: None,
            push: true,
            target: RemoteTarget::Shared,
            scope: ScopeConfig::Literal("pr-1".into()),
            max_artifact_size: Some(5),
        })
        .unwrap();
        let config: RemoteCacheConfiguration =
            rmp_serde::from_slice(&bytes).unwrap();
        assert!(config.push);
        assert_eq!(config.target, RemoteTarget::Shared);
        assert_eq!(config.max_artifact_size, Some(5));
        assert_eq!(config.signing_key, None);
        assert_eq!(config.verify, None);
        assert_eq!(config.trusted_keys, None);
    }

    #[test]
    fn new_settings_round_trip_through_msgpack_and_json() {
        let config = RemoteCacheConfiguration {
            api_key: "k".into(),
            api_base_url: "http://h".into(),
            tenant_code: "t".into(),
            organization_code: "o".into(),
            workspace_code: "w".into(),
            environment_code: None,
            push: true,
            target: RemoteTarget::Shared,
            scope: ScopeConfig::Literal("pr-1".into()),
            max_artifact_size: Some(1024),
            signing_key: Some(SigningKeyConfiguration {
                id: "0123456789abcdef".into(),
                private_key_env: Some("KEY".into()),
                private_key_file: None,
            }),
            verify: Some(VerifyMode::Require),
            trusted_keys: Some(vec!["0123456789abcdef".into()]),
        };
        let bytes = rmp_serde::to_vec(&config).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<RemoteCacheConfiguration>(&bytes).unwrap(),
            config
        );
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""target":"shared""#));
        assert!(json.contains(r#""scope":"pr-1""#));
        assert_eq!(
            serde_json::from_str::<RemoteCacheConfiguration>(&json).unwrap(),
            config
        );
    }

    #[test]
    fn a_misspelled_field_or_bad_value_is_rejected() {
        let base = |extra: &str| {
            format!(
                r#"{{"api_key":"k","api_base_url":"h","tenant_code":"t","organization_code":"o","workspace_code":"w"{extra}}}"#
            )
        };
        for bad in [
            r#","pushh":true"#,
            r#","target":"everywhere""#,
            r#","scope":"../x""#,
            r#","scope":"""#,
            r#","scope":"a/b""#,
            r#","max_artifact_size":-1"#,
        ] {
            assert!(
                serde_json::from_str::<RemoteCacheConfiguration>(&base(bad))
                    .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn scopes_parse_the_reserved_words_and_literals() {
        assert_eq!(ScopeConfig::parse("auto"), Ok(ScopeConfig::Auto));
        assert_eq!(ScopeConfig::parse("run"), Ok(ScopeConfig::Run));
        assert_eq!(
            ScopeConfig::parse("Release_1.2"),
            Ok(ScopeConfig::Literal("Release_1.2".into()))
        );
        for bad in ["", "-a", ".a", "a b", "é", "Auto", "RUN", &"x".repeat(65)]
        {
            assert!(ScopeConfig::parse(bad).is_err(), "{bad:?}");
        }
        assert!(ScopeConfig::parse(&"x".repeat(64)).is_ok());
    }
}
