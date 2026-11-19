//! Ed25519 signatures for shared cache entries (RFC 0030, section 4.5).
//!
//! A publisher signs a message that binds an entry to its place and content;
//! a verifier holds only public keys, so verifying cannot forge. The server
//! never sees a key: it stores the signature string and returns it.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// Domain separation for the signed message.
const DOMAIN: &[u8] = b"omni-cache-entry-sig-v1";
/// A key id is the first 16 hex characters of the SHA-256 of the public key.
const KEY_ID_LEN: usize = 16;
/// `<16 hex>:<88 base64 characters>`
const SIGNATURE_WIRE_LEN: usize = KEY_ID_LEN + 1 + 88;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SigningError {
    #[error("a key id is exactly 16 lowercase hexadecimal characters")]
    InvalidKeyId,
    #[error("a public key is 32 bytes of standard base64")]
    InvalidPublicKey,
    #[error("this public key is weak (small order) and cannot be trusted")]
    WeakPublicKey,
    #[error(
        "the key id {given} does not match the public key (expected {expected})"
    )]
    KeyIdMismatch { given: String, expected: String },
    #[error("a private key is 32 bytes of standard base64")]
    InvalidPrivateKey,
    #[error(
        "the signature is not `<key id>:<base64>` with a 64 byte signature"
    )]
    InvalidSignature,
    #[error("the operating system could not provide random bytes")]
    Randomness,
}

/// Lowercase hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The id of a public key. A 64 bit id is not a security boundary: lookup finds
/// the trusted key and the signature is then verified against that key, so a
/// colliding id only ever selects a key the verifier already trusts.
pub fn key_id(public_key: &[u8; 32]) -> String {
    hex(&Sha256::digest(public_key))[..KEY_ID_LEN].to_owned()
}

/// True for a well formed key id: 16 lowercase hexadecimal characters.
pub fn is_key_id(text: &str) -> bool {
    text.len() == KEY_ID_LEN
        && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A public key the verifier trusts, with the id it is known by.
#[derive(Clone, PartialEq, Eq)]
pub struct TrustedKey {
    id: String,
    key: VerifyingKey,
}

impl fmt::Debug for TrustedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustedKey").field("id", &self.id).finish()
    }
}

impl TrustedKey {
    /// Builds a trusted key from its configured id and base64 public key. The
    /// id must be the one derived from the key, so an id is bound to exactly one
    /// key and never reused for different material.
    pub fn parse(
        id: &str,
        public_key_base64: &str,
    ) -> Result<Self, SigningError> {
        if !is_key_id(id) {
            return Err(SigningError::InvalidKeyId);
        }
        let bytes: [u8; 32] = STANDARD
            .decode(public_key_base64)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or(SigningError::InvalidPublicKey)?;
        let key = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| SigningError::InvalidPublicKey)?;
        if key.is_weak() {
            return Err(SigningError::WeakPublicKey);
        }
        let expected = key_id(&bytes);
        if expected != id {
            return Err(SigningError::KeyIdMismatch {
                given: id.to_owned(),
                expected,
            });
        }
        Ok(Self {
            id: id.to_owned(),
            key,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

/// The bytes a publisher signs and a verifier checks. Every field is prefixed
/// by its length as a big-endian `u32`, so no bytes in a field can move into
/// the next. `namespace` is always the constant `shared`: only shared entries
/// are signed. `sha256` is the hash the verifier computed itself.
pub fn signed_message(
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
    digest: &str,
    sha256: &[u8; 32],
) -> Vec<u8> {
    let mut message = Vec::new();
    for field in [
        DOMAIN,
        tenant.as_bytes(),
        org.as_bytes(),
        ws.as_bytes(),
        env.as_bytes(),
        b"shared",
        digest.as_bytes(),
        &sha256[..],
    ] {
        message.extend_from_slice(&(field.len() as u32).to_be_bytes());
        message.extend_from_slice(field);
    }
    message
}

/// A signing key held in memory. It is zeroized on drop, its `Debug` shows only
/// the id, and it has no serializer, so it cannot leak through logs or config.
pub struct SigningHandle {
    id: String,
    key: SigningKey,
}

impl fmt::Debug for SigningHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl SigningHandle {
    /// From the base64 text of a 32 byte seed. Surrounding whitespace (a
    /// trailing newline in a key file) is ignored.
    pub fn from_base64(private_key: &str) -> Result<Self, SigningError> {
        let seed: Zeroizing<Vec<u8>> = Zeroizing::new(
            STANDARD
                .decode(private_key.trim())
                .map_err(|_| SigningError::InvalidPrivateKey)?,
        );
        let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
            <[u8; 32]>::try_from(seed.as_slice())
                .map_err(|_| SigningError::InvalidPrivateKey)?,
        );
        let key = SigningKey::from_bytes(&seed);
        let id = key_id(key.verifying_key().as_bytes());
        Ok(Self { id, key })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(self.key.verifying_key().as_bytes())
    }

    /// The wire form `<key id>:<base64 signature>` of a signature over
    /// `message`.
    pub fn sign(&self, message: &[u8]) -> String {
        let signature = self.key.sign(message);
        format!("{}:{}", self.id, STANDARD.encode(signature.to_bytes()))
    }
}

/// A freshly generated key pair, for `omni cache keygen`.
pub struct Keypair {
    pub id: String,
    pub public_key: String,
    pub private_key: Zeroizing<String>,
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// A new key pair from the operating system's random number generator.
pub fn generate_keypair() -> Result<Keypair, SigningError> {
    use ring::rand::{SecureRandom as _, SystemRandom};
    let mut seed = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(&mut seed[..])
        .map_err(|_| SigningError::Randomness)?;
    let private_key = Zeroizing::new(STANDARD.encode(&seed[..]));
    let handle = SigningHandle::from_base64(&private_key)?;
    Ok(Keypair {
        id: handle.id().to_owned(),
        public_key: handle.public_key_base64(),
        private_key,
    })
}

/// Why a signature was not accepted. Never carries signature bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("the entry is not signed")]
    Unsigned,
    #[error("the signature is malformed")]
    Malformed,
    #[error("the entry is signed by a key that is not trusted")]
    UnknownKey,
    #[error("the signature does not verify")]
    BadSignature,
}

/// Splits `<key id>:<base64>` strictly: a 16 lowercase hex id, a colon, and
/// exactly 88 characters of padded standard base64 that decode to 64 bytes.
fn parse_wire(wire: &str) -> Result<(&str, [u8; 64]), Rejection> {
    if wire.len() != SIGNATURE_WIRE_LEN {
        return Err(Rejection::Malformed);
    }
    let (id, rest) = wire.split_at(KEY_ID_LEN);
    let encoded = rest.strip_prefix(':').ok_or(Rejection::Malformed)?;
    if !is_key_id(id) {
        return Err(Rejection::Malformed);
    }
    let bytes: [u8; 64] = STANDARD
        .decode(encoded)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(Rejection::Malformed)?;
    Ok((id, bytes))
}

/// True when `wire` is a well formed signature string. The server uses this to
/// store only strings a client can read back.
pub fn is_well_formed_signature(wire: &str) -> bool {
    parse_wire(wire).is_ok()
}

/// Verifies `wire` over `message` against the trusted keys. `verify_strict`
/// also refuses small-order components.
pub fn verify(
    trusted: &[TrustedKey],
    wire: Option<&str>,
    message: &[u8],
) -> Result<String, Rejection> {
    let wire = wire.ok_or(Rejection::Unsigned)?;
    let (id, bytes) = parse_wire(wire)?;
    let key = trusted
        .iter()
        .find(|k| k.id == id)
        .ok_or(Rejection::UnknownKey)?;
    key.key
        .verify_strict(message, &Signature::from_bytes(&bytes))
        .map_err(|_| Rejection::BadSignature)?;
    Ok(key.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: [u8; 32] = [7; 32];

    fn pair() -> (SigningHandle, TrustedKey) {
        let kp = generate_keypair().unwrap();
        let handle = SigningHandle::from_base64(&kp.private_key).unwrap();
        let trusted = TrustedKey::parse(&kp.id, &kp.public_key).unwrap();
        (handle, trusted)
    }

    fn message() -> Vec<u8> {
        signed_message("t", "o", "w", "e", "digest1", &SHA)
    }

    #[test]
    fn the_message_has_a_pinned_encoding() {
        // Each field: u32 big-endian length, then the bytes.
        let expected = {
            let mut v = Vec::new();
            for f in [
                &b"omni-cache-entry-sig-v1"[..],
                b"t",
                b"o",
                b"w",
                b"e",
                b"shared",
                b"digest1",
                &SHA[..],
            ] {
                v.extend((f.len() as u32).to_be_bytes());
                v.extend(f);
            }
            v
        };
        assert_eq!(message(), expected);
        assert_eq!(
            hex(&Sha256::digest(message())),
            "9737bbd65ae967950f88c17f7c0a6ad548e335b23aed63b3c38726157d8f9012",
        );
    }

    #[test]
    fn rfc_8032_test_vector_1_signs_and_verifies() {
        // RFC 8032 section 7.1, TEST 1 (empty message).
        let seed =
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
        let public =
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let expected = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";
        let unhex = |s: &str| -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };
        let handle =
            SigningHandle::from_base64(&STANDARD.encode(unhex(seed))).unwrap();
        assert_eq!(
            hex(&STANDARD.decode(handle.public_key_base64()).unwrap()),
            public
        );
        let wire = handle.sign(b"");
        let (_, sig) = parse_wire(&wire).unwrap();
        assert_eq!(hex(&sig), expected);
    }

    #[test]
    fn a_signature_verifies_only_for_the_exact_message() {
        let (handle, trusted) = pair();
        let wire = handle.sign(&message());
        assert_eq!(
            verify(std::slice::from_ref(&trusted), Some(&wire), &message()),
            Ok(trusted.id().to_owned())
        );
        // Changing any one field of the message must fail.
        let others = [
            signed_message("t2", "o", "w", "e", "digest1", &SHA),
            signed_message("t", "o2", "w", "e", "digest1", &SHA),
            signed_message("t", "o", "w2", "e", "digest1", &SHA),
            signed_message("t", "o", "w", "e2", "digest1", &SHA),
            signed_message("t", "o", "w", "e", "digest2", &SHA),
            signed_message("t", "o", "w", "e", "digest1", &[8; 32]),
        ];
        for other in others {
            assert_eq!(
                verify(std::slice::from_ref(&trusted), Some(&wire), &other),
                Err(Rejection::BadSignature)
            );
        }
    }

    #[test]
    fn length_prefixes_stop_fields_shifting_into_each_other() {
        assert_ne!(
            signed_message("ab", "c", "w", "e", "d", &SHA),
            signed_message("a", "bc", "w", "e", "d", &SHA)
        );
        assert_ne!(
            signed_message("t", "o", "w", "e", "d", &SHA),
            signed_message("t", "o", "w", "e", "dshared", &SHA)
        );
    }

    #[test]
    fn a_signature_under_another_keys_id_does_not_verify() {
        let (a, trusted_a) = pair();
        let (_, trusted_b) = pair();
        // A valid signature by key A, relabelled with B's id.
        let wire = a.sign(&message());
        let (_, tail) = wire.split_once(':').unwrap();
        let relabelled = format!("{}:{tail}", trusted_b.id());
        assert_eq!(
            verify(
                &[trusted_a.clone(), trusted_b],
                Some(&relabelled),
                &message()
            ),
            Err(Rejection::BadSignature)
        );
        // Not trusting the signer at all.
        assert_eq!(
            verify(&[], Some(&wire), &message()),
            Err(Rejection::UnknownKey)
        );
        assert_eq!(
            verify(&[trusted_a], None, &message()),
            Err(Rejection::Unsigned)
        );
    }

    #[test]
    fn the_wire_grammar_is_strict() {
        let (handle, trusted) = pair();
        let wire = handle.sign(&message());
        let (id, b64) = wire.split_once(':').unwrap();
        let bad = [
            // Keys are random: an id of only digits has no uppercase form, so
            // swap in a character that is never lowercase hex.
            format!("G{}:{b64}", &id[1..]),
            format!("{id}:{}", b64.trim_end_matches('=')),
            format!("{id}:{b64}A"),
            format!("{id}:{}", &b64[..b64.len() - 4]),
            format!("{id} {b64}"),
            format!("{}:{b64}", &id[..15]),
            // A URL-safe alphabet character, whatever the random bytes were.
            format!("{id}:-{}", &b64[1..]),
            String::new(),
            format!("{id}:{}", "A".repeat(88)),
        ];
        for text in &bad {
            assert!(!is_well_formed_signature(text), "{text:?}");
            assert!(
                verify(std::slice::from_ref(&trusted), Some(text), &message())
                    .is_err(),
                "{text:?}"
            );
        }
        // Non-canonical trailing bits: the last data character before `==`
        // carries two unused bits that must be zero.
        let last = b64.trim_end_matches('=');
        let tweaked = {
            let mut chars: Vec<char> = last.chars().collect();
            let c = chars.pop().unwrap();
            let bumped = if c == 'A' { 'B' } else { 'A' };
            chars.push(bumped);
            format!("{id}:{}==", chars.into_iter().collect::<String>())
        };
        if tweaked != wire {
            // Only a character whose unused bits are non-zero is non-canonical;
            // either way it must never verify.
            assert!(
                verify(
                    std::slice::from_ref(&trusted),
                    Some(&tweaked),
                    &message()
                )
                .is_err()
            );
        }
        // Well formed but not a signature by this key.
        let zero = format!("{id}:{}", STANDARD.encode([0u8; 64]));
        assert!(is_well_formed_signature(&zero));
        assert_eq!(
            verify(std::slice::from_ref(&trusted), Some(&zero), &message()),
            Err(Rejection::BadSignature)
        );
        assert!(is_well_formed_signature(&wire));
    }

    #[test]
    fn a_trusted_key_is_bound_to_its_derived_id() {
        let kp = generate_keypair().unwrap();
        assert!(TrustedKey::parse(&kp.id, &kp.public_key).is_ok());
        let other = generate_keypair().unwrap();
        assert!(matches!(
            TrustedKey::parse(&other.id, &kp.public_key),
            Err(SigningError::KeyIdMismatch { .. })
        ));
        for bad_id in ["", "ABCDEF0123456789", "0123", &"a".repeat(17)] {
            assert_eq!(
                TrustedKey::parse(bad_id, &kp.public_key),
                Err(SigningError::InvalidKeyId)
            );
        }
        for bad_key in ["", "AAAA", "not base64!", &STANDARD.encode([1u8; 31])]
        {
            assert_eq!(
                TrustedKey::parse(&kp.id, bad_key),
                Err(SigningError::InvalidPublicKey)
            );
        }
    }

    #[test]
    fn a_weak_public_key_is_refused() {
        // The identity point (compressed 01 00 .. 00) has small order.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let id = key_id(&identity);
        assert_eq!(
            TrustedKey::parse(&id, &STANDARD.encode(identity)),
            Err(SigningError::WeakPublicKey)
        );
    }

    #[test]
    fn private_keys_never_appear_in_debug_output() {
        let kp = generate_keypair().unwrap();
        let handle =
            SigningHandle::from_base64(&format!("{}\n", *kp.private_key))
                .unwrap();
        for text in [format!("{handle:?}"), format!("{kp:?}")] {
            assert!(!text.contains(&*kp.private_key), "{text}");
        }
        assert_eq!(handle.id(), kp.id);
        assert_eq!(
            SigningHandle::from_base64("not-a-key").err(),
            Some(SigningError::InvalidPrivateKey)
        );
        assert_eq!(
            SigningHandle::from_base64(&STANDARD.encode([0u8; 31])).err(),
            Some(SigningError::InvalidPrivateKey)
        );
    }

    #[test]
    fn two_generated_pairs_differ() {
        let a = generate_keypair().unwrap();
        let b = generate_keypair().unwrap();
        assert_ne!(a.id, b.id);
        assert_ne!(*a.private_key, *b.private_key);
    }
}
