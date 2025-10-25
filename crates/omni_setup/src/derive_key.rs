use ring::pbkdf2;
use std::num::NonZeroU32;

pub fn derive_key_from_seed(seed: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];

    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(100_000).unwrap(),
        salt,
        seed.as_bytes(),
        &mut key,
    );
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_not_zeroed(bytes: &[u8]) -> bool {
        bytes.iter().any(|b| *b != 0)
    }

    #[test]
    fn test_derive_key_from_seed() {
        let seed = "0123456789abcdef";
        let salt = b"salt";
        let key = derive_key_from_seed(seed, salt);

        assert!(is_not_zeroed(&key));
    }
}
