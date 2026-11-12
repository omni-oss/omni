use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum HashAlgorithm {
    None,
    Sha256,
    Sha512,
    HmacSha256,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum HashEncoding {
    Hex,
    Base64,
}

/// How a presented API key is transformed before it is bound to a query.
/// Only deterministic hashes are supported because the lookup is by value.
#[derive(Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ApiKeyHashConfiguration {
    pub algorithm: HashAlgorithm,
    pub encoding: HashEncoding,
    /// Required for `hmac-sha256`.
    #[serde(default)]
    pub secret: Option<String>,
}

impl std::fmt::Debug for ApiKeyHashConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyHashConfiguration")
            .field("algorithm", &self.algorithm)
            .field("encoding", &self.encoding)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl Default for ApiKeyHashConfiguration {
    fn default() -> Self {
        Self {
            algorithm: HashAlgorithm::Sha256,
            encoding: HashEncoding::Hex,
            secret: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashConfigError {
    #[error("hmac-sha256 requires a secret")]
    MissingSecret,
}

impl ApiKeyHashConfiguration {
    pub fn validate(&self) -> Result<(), HashConfigError> {
        if self.algorithm == HashAlgorithm::HmacSha256
            && self.secret.as_deref().is_none_or(str::is_empty)
        {
            return Err(HashConfigError::MissingSecret);
        }
        Ok(())
    }

    /// Returns the value to bind for `key`.
    pub fn apply(&self, key: &str) -> String {
        let bytes: Vec<u8> = match self.algorithm {
            HashAlgorithm::None => return key.to_string(),
            HashAlgorithm::Sha256 => Sha256::digest(key.as_bytes()).to_vec(),
            HashAlgorithm::Sha512 => Sha512::digest(key.as_bytes()).to_vec(),
            HashAlgorithm::HmacSha256 => {
                let secret = self.secret.as_deref().unwrap_or_default();
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                    .expect("hmac accepts keys of any length");
                mac.update(key.as_bytes());
                mac.finalize().into_bytes().to_vec()
            }
        };
        match self.encoding {
            HashEncoding::Hex => {
                bytes.iter().map(|b| format!("{b:02x}")).collect()
            }
            HashEncoding::Base64 => STANDARD.encode(bytes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(a: HashAlgorithm, e: HashEncoding) -> ApiKeyHashConfiguration {
        ApiKeyHashConfiguration {
            algorithm: a,
            encoding: e,
            secret: Some("pepper".into()),
        }
    }

    #[test]
    fn known_vectors() {
        use HashAlgorithm::*;
        use HashEncoding::*;
        assert_eq!(cfg(None, Hex).apply("abc"), "abc");
        assert_eq!(
            cfg(Sha256, Hex).apply("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            cfg(Sha256, Base64).apply("abc"),
            "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0="
        );
        assert_eq!(cfg(Sha512, Hex).apply("abc").len(), 128);
        assert_ne!(
            cfg(HmacSha256, Hex).apply("abc"),
            cfg(Sha256, Hex).apply("abc")
        );
    }

    #[test]
    fn hmac_depends_on_secret_and_requires_one() {
        let a = cfg(HashAlgorithm::HmacSha256, HashEncoding::Hex);
        let mut b = a.clone();
        b.secret = Some("other".into());
        assert_ne!(a.apply("k"), b.apply("k"));
        b.secret = None;
        assert!(b.validate().is_err());
        assert!(a.validate().is_ok());
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let c = cfg(HashAlgorithm::HmacSha256, HashEncoding::Hex);
        assert!(!format!("{c:?}").contains("pepper"));
    }

    #[test]
    fn default_is_sha256_hex() {
        let d = ApiKeyHashConfiguration::default();
        assert_eq!(d.apply("abc").len(), 64);
    }
}
