use std::io::Read;

use derive_new::new;
use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

const NONCE_LEN: usize = 12; // GCM standard
const TAG_LEN: usize = 16; // AES-GCM tag length is always 16 bytes

pub fn encrypt<RInput: Read, RKey: Read>(
    mut input: RInput,
    mut key: RKey,
) -> Result<Vec<u8>, CryptoError> {
    // Read key
    let mut key_buff = vec![];
    let n = key.read_to_end(&mut key_buff)?;
    let key_slice = &key_buff[..n];

    // Create sealing key
    let unbound_key = aead::UnboundKey::new(&aead::AES_256_GCM, key_slice)
        .map_err(|e| {
            CryptoErrorInner::InvalidKeyLength(
                n,
                eyre::Report::msg(e.to_string()),
            )
        })?;
    let sealing_key = aead::LessSafeKey::new(unbound_key);

    // Generate random nonce
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes).map_err(|e| {
        CryptoErrorInner::FailedToFillBytes(eyre::Report::msg(e.to_string()))
    })?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    // Read input to buffer
    let mut buffer = vec![];
    input.read_to_end(&mut buffer)?;

    // Encrypt in place (we’ll append the tag ourselves)
    let tag = sealing_key
        .seal_in_place_separate_tag(nonce, aead::Aad::empty(), &mut buffer)
        .map_err(|e| {
            CryptoErrorInner::FailedToSeal(eyre::Report::msg(e.to_string()))
        })?;
    buffer.extend_from_slice(tag.as_ref());

    // Final output: [nonce][ciphertext+tag]
    let mut out = Vec::with_capacity(NONCE_LEN + buffer.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&buffer);

    Ok(out)
}

pub fn decrypt<RInput: Read, RKey: Read>(
    mut input: RInput,
    mut key: RKey,
) -> Result<Vec<u8>, CryptoError> {
    // Read ciphertext
    let mut input_buff = vec![];
    input.read_to_end(&mut input_buff)?;
    if input_buff.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoErrorInner::FailedToOpen(eyre::eyre!(
            "ciphertext too short"
        ))
        .into());
    }

    // Read key
    let mut key_buff = vec![];
    let n = key.read_to_end(&mut key_buff)?;
    let key_slice = &key_buff[..n];

    // Split nonce and ciphertext
    let (nonce_bytes, ciphertext_with_tag) = input_buff.split_at(NONCE_LEN);
    let nonce =
        aead::Nonce::assume_unique_for_key(nonce_bytes.try_into().unwrap());

    // Create opening key
    let unbound_key = aead::UnboundKey::new(&aead::AES_256_GCM, key_slice)
        .map_err(|e| {
            CryptoErrorInner::InvalidKeyLength(
                n,
                eyre::Report::msg(e.to_string()),
            )
        })?;
    let opening_key = aead::LessSafeKey::new(unbound_key);

    // Decrypt in place
    let mut buffer = ciphertext_with_tag.to_vec();
    let plaintext = opening_key
        .open_in_place(nonce, aead::Aad::empty(), &mut buffer)
        .map_err(|e| {
            CryptoErrorInner::FailedToOpen(eyre::Report::msg(e.to_string()))
        })?;

    // open_in_place() returns a slice of valid plaintext, trimmed of padding
    Ok(plaintext.to_vec())
}

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct CryptoError(pub(crate) CryptoErrorInner);

impl CryptoError {
    #[allow(unused)]
    pub fn kind(&self) -> CryptoErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<CryptoErrorInner>> From<T> for CryptoError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(CryptoErrorKind))]
pub(crate) enum CryptoErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("invalid key length: {0}")]
    InvalidKeyLength(usize, #[source] eyre::Report),

    #[error("failed to seal")]
    FailedToSeal(#[source] eyre::Report),

    #[error("failed to open")]
    FailedToOpen(#[source] eyre::Report),

    #[error("failed to open")]
    FailedToFillBytes(#[source] eyre::Report),
}

#[cfg(test)]
mod tests {
    use rand::RngCore;

    use super::*;

    fn get_random_bytes(len: usize) -> Vec<u8> {
        let mut rng = rand::rng();
        let mut bytes = vec![0u8; len];
        rng.fill_bytes(&mut bytes[..]);
        bytes
    }

    #[test]
    fn test_encrypt_decrypt() {
        let key = get_random_bytes(32);
        let input = b"Hello, world!";

        let encrypted =
            encrypt(&input[..], &key[..]).expect("should be able to encrypt");
        let decrypted = decrypt(&encrypted[..], &key[..])
            .expect("should be able to decrypt");

        assert_eq!(input, decrypted.as_slice());
    }
}
