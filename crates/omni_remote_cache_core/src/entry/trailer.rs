use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Closes every sealed object.
pub const MAGIC: &[u8; 8] = b"OMNIENT1";

/// The fixed end of an object: a big-endian `u32` trailer length, then [`MAGIC`].
pub const FOOTER_LEN: usize = 4 + MAGIC.len();

/// The largest trailer accepted, written or read.
pub const MAX_TRAILER_LEN: usize = 64 * 1024;

/// How many bytes from the end of an object a reader asks for.
pub const TAIL_READ_LEN: u64 = (MAX_TRAILER_LEN + FOOTER_LEN) as u64;

/// The only trailer version this code writes or accepts.
pub const TRAILER_VERSION: u32 = 1;

/// Who stored an entry, as the authenticator reported it. Never client data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailerCredential {
    pub kind: String,
    pub id: String,
}

/// What the server records about an entry. It identifies who stored the entry,
/// so it must not be logged or shown to callers who lack the operator scopes.
/// Every field comes from the
/// authenticated caller, the validated request or the server's own
/// measurements, except `signature`, which is opaque to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub version: u32,
    pub writer_credential: TrailerCredential,
    pub writer_principal: String,
    pub namespace: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub stored_at: OffsetDateTime,
    /// Artifact bytes, not the object size.
    pub size: u64,
    /// Lowercase hex SHA-256 of the artifact bytes.
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
}

/// Why bytes are not a well-formed sealed object. Callers treat every one as
/// a cache miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error("object is shorter than the entry footer")]
    TooShort,
    #[error("object does not end with the entry magic")]
    BadMagic,
    #[error("trailer length is zero, too large or past the start of the read")]
    BadTrailerLength,
    #[error("trailer is not valid")]
    Malformed,
    #[error("trailer version is not supported")]
    UnsupportedVersion,
    #[error("trailer size does not match the object length")]
    SizeMismatch,
    #[error("trailer is too large to write")]
    TrailerTooLarge,
}

/// True for 64 lowercase hexadecimal characters.
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Lowercase hex of `bytes`.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

impl Trailer {
    /// The bytes appended after the artifact: the msgpack trailer, its length
    /// and the magic.
    pub fn encode(&self) -> Result<Vec<u8>, EntryError> {
        if self.version != TRAILER_VERSION || !is_sha256_hex(&self.sha256) {
            return Err(EntryError::Malformed);
        }
        let mut bytes =
            rmp_serde::to_vec_named(self).map_err(|_| EntryError::Malformed)?;
        let length = u32::try_from(bytes.len())
            .ok()
            .filter(|_| bytes.len() <= MAX_TRAILER_LEN)
            .ok_or(EntryError::TrailerTooLarge)?;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(MAGIC);
        Ok(bytes)
    }

    /// Reads the trailer from `tail`, which must be exactly the last
    /// `min(object_len, TAIL_READ_LEN)` bytes of an object that is `object_len`
    /// bytes long (the whole object when it is shorter than the read). Checks everything a reader relies on: the magic, the length, the
    /// version, the hash format, and that the trailer's `size` plus the trailer
    /// and footer account for exactly the object.
    pub fn parse_tail(
        tail: &[u8],
        object_len: u64,
    ) -> Result<Parsed, EntryError> {
        if tail.len() < FOOTER_LEN {
            return Err(EntryError::TooShort);
        }
        // `tail` is the end of the object, so it cannot be longer than it.
        if tail.len() as u64 > object_len {
            return Err(EntryError::BadTrailerLength);
        }
        let (rest, footer) = tail.split_at(tail.len() - FOOTER_LEN);
        let (length, magic) = footer.split_at(4);
        if magic != MAGIC {
            return Err(EntryError::BadMagic);
        }
        let length = u32::from_be_bytes(length.try_into().expect("4 bytes"));
        let length = usize::try_from(length)
            .ok()
            .filter(|n| (1..=MAX_TRAILER_LEN).contains(n) && *n <= rest.len())
            .ok_or(EntryError::BadTrailerLength)?;
        let bytes = &rest[rest.len() - length..];
        // The whole region must be exactly one trailer: bytes after it would let
        // one trailer have several encodings, which a reader that compares or a
        // later signature that covers the bytes must not allow.
        let mut decoder = rmp_serde::Deserializer::new(bytes);
        let trailer = Trailer::deserialize(&mut decoder)
            .map_err(|_| EntryError::Malformed)?;
        if !decoder.into_inner().is_empty() {
            return Err(EntryError::Malformed);
        }
        if trailer.version != TRAILER_VERSION {
            return Err(EntryError::UnsupportedVersion);
        }
        if !is_sha256_hex(&trailer.sha256) {
            return Err(EntryError::Malformed);
        }
        let accounted = trailer
            .size
            .checked_add(length as u64)
            .and_then(|n| n.checked_add(FOOTER_LEN as u64));
        if accounted != Some(object_len) {
            return Err(EntryError::SizeMismatch);
        }
        Ok(Parsed {
            trailer,
            trailer_len: length,
        })
    }
}

/// A trailer that passed [`Trailer::parse_tail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub trailer: Trailer,
    pub trailer_len: usize,
}

impl Parsed {
    /// The footer-inclusive bytes at the end of the object: trailer, length
    /// and magic. A reader holds them back from the body and compares them with
    /// what it first read, so a different object is never mixed in.
    pub fn sealed_len(&self) -> usize {
        self.trailer_len + FOOTER_LEN
    }

    /// Whether `held_back`, the last bytes a reader streamed from the object,
    /// are exactly the trailer, length and magic that `tail` (read earlier) ends
    /// with. A different object, such as one re-published between the two reads,
    /// always differs here, because its trailer differs.
    pub fn matches_tail(&self, tail: &[u8], held_back: &[u8]) -> bool {
        let n = self.sealed_len();
        held_back.len() == n
            && tail.len() >= n
            && tail[tail.len() - n..] == *held_back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trailer(size: u64) -> Trailer {
        Trailer {
            version: TRAILER_VERSION,
            writer_credential: TrailerCredential {
                kind: "api_key".into(),
                id: "0123456789abcdef".into(),
            },
            writer_principal: "owner".into(),
            namespace: "shared".into(),
            scope: None,
            stored_at: OffsetDateTime::UNIX_EPOCH,
            size,
            sha256: "a".repeat(64),
            signature: None,
        }
    }

    /// An object: `artifact` bytes followed by the encoded trailer.
    fn object(artifact: &[u8]) -> (Vec<u8>, Trailer) {
        let trailer = trailer(artifact.len() as u64);
        let mut object = artifact.to_vec();
        object.extend(trailer.encode().unwrap());
        (object, trailer)
    }

    #[test]
    fn a_sealed_object_round_trips_from_its_tail() {
        for artifact in [&b""[..], b"x", b"hello world", &[0u8; 5000]] {
            let (object, trailer) = object(artifact);
            let parsed =
                Trailer::parse_tail(&object, object.len() as u64).unwrap();
            assert_eq!(parsed.trailer, trailer);
            assert_eq!(
                parsed.sealed_len(),
                object.len() - artifact.len(),
                "the sealed part is everything after the artifact"
            );
        }
    }

    #[test]
    fn only_the_end_of_a_long_object_is_needed() {
        let artifact = vec![7u8; 200_000];
        let (object, trailer) = object(&artifact);
        let tail = &object[object.len() - TAIL_READ_LEN as usize..];
        let parsed = Trailer::parse_tail(tail, object.len() as u64).unwrap();
        assert_eq!(parsed.trailer, trailer);
    }

    #[test]
    fn an_artifact_that_ends_in_the_magic_cannot_confuse_the_reader() {
        // The footer is always the last twelve bytes, so bytes inside the
        // artifact that look like a footer are just artifact bytes.
        let mut artifact = b"payload".to_vec();
        artifact.extend_from_slice(&1u32.to_be_bytes());
        artifact.extend_from_slice(MAGIC);
        let (object, trailer) = object(&artifact);
        let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
        assert_eq!(parsed.trailer, trailer);
        assert_eq!(parsed.trailer.size, artifact.len() as u64);
    }

    #[test]
    fn extra_fields_are_ignored_and_optional_fields_default() {
        #[derive(Serialize)]
        struct Future {
            version: u32,
            writer_credential: TrailerCredential,
            writer_principal: &'static str,
            namespace: &'static str,
            #[serde(with = "time::serde::rfc3339")]
            stored_at: OffsetDateTime,
            size: u64,
            sha256: String,
            added_later: &'static str,
        }
        let bytes = rmp_serde::to_vec_named(&Future {
            version: TRAILER_VERSION,
            writer_credential: TrailerCredential {
                kind: "oidc".into(),
                id: "id".into(),
            },
            writer_principal: "p",
            namespace: "shared",
            stored_at: OffsetDateTime::UNIX_EPOCH,
            size: 3,
            sha256: "b".repeat(64),
            added_later: "ignored",
        })
        .unwrap();
        let mut object = b"abc".to_vec();
        object.extend_from_slice(&bytes);
        object.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        object.extend_from_slice(MAGIC);
        let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
        assert_eq!(parsed.trailer.scope, None);
        assert_eq!(parsed.trailer.signature, None);
        assert_eq!(parsed.trailer.writer_credential.kind, "oidc");
    }

    #[test]
    fn fields_are_written_by_name_as_a_map() {
        let encoded = trailer(1).encode().unwrap();
        let body = &encoded[..encoded.len() - FOOTER_LEN];
        // A msgpack map (fixmap, map16 or map32), not an array of positions, so
        // a field can be added later without breaking older readers.
        assert!(
            matches!(body[0], 0x80..=0x8f | 0xde | 0xdf),
            "first byte {:#x}",
            body[0]
        );
        assert!(body.windows(6).any(|w| w == b"sha256"));
    }

    #[test]
    fn malformed_objects_are_each_rejected() {
        let (good, _) = object(b"artifact");
        let len = good.len() as u64;
        let footer_start = good.len() - FOOTER_LEN;
        let with_footer = |length: u32, magic: &[u8; 8]| {
            let mut bytes = good[..footer_start].to_vec();
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(magic);
            bytes
        };
        let real_len = u32::from_be_bytes(
            good[footer_start..footer_start + 4].try_into().unwrap(),
        );

        let cases: Vec<(&str, Vec<u8>, u64, EntryError)> = vec![
            ("empty", vec![], 0, EntryError::TooShort),
            ("eleven bytes", vec![0; 11], 11, EntryError::TooShort),
            ("no magic", vec![0; 40], 40, EntryError::BadMagic),
            (
                "a legacy tar.gz",
                b"\x1f\x8b\x08\x00 plain gzip bytes, no footer".to_vec(),
                35,
                EntryError::BadMagic,
            ),
            (
                "zero length",
                with_footer(0, MAGIC),
                len,
                EntryError::BadTrailerLength,
            ),
            (
                "length past the bytes read",
                with_footer(real_len + 1000, MAGIC),
                len,
                EntryError::BadTrailerLength,
            ),
            (
                "length over the limit",
                with_footer((MAX_TRAILER_LEN + 1) as u32, MAGIC),
                len,
                EntryError::BadTrailerLength,
            ),
            (
                "length u32 max",
                with_footer(u32::MAX, MAGIC),
                len,
                EntryError::BadTrailerLength,
            ),
            (
                "length one short cuts the msgpack",
                with_footer(real_len - 1, MAGIC),
                len,
                EntryError::Malformed,
            ),
            (
                "garbage msgpack",
                {
                    let mut bytes = b"artifact".to_vec();
                    bytes.extend_from_slice(&[0xc1; 10]);
                    bytes.extend_from_slice(&10u32.to_be_bytes());
                    bytes.extend_from_slice(MAGIC);
                    bytes
                },
                8 + 10 + FOOTER_LEN as u64,
                EntryError::Malformed,
            ),
        ];
        for (name, bytes, object_len, want) in cases {
            assert_eq!(
                Trailer::parse_tail(&bytes, object_len).unwrap_err(),
                want,
                "{name}"
            );
        }
    }

    #[test]
    fn a_trailer_that_lies_about_the_size_is_rejected() {
        // The trailer claims a different artifact size than the object holds.
        let artifact = b"artifact";
        for lie in [
            0,
            artifact.len() as u64 - 1,
            artifact.len() as u64 + 1,
            u64::MAX,
        ] {
            let mut t = trailer(lie);
            t.sha256 = "c".repeat(64);
            let mut object = artifact.to_vec();
            object.extend(t.encode().unwrap());
            assert_eq!(
                Trailer::parse_tail(&object, object.len() as u64).unwrap_err(),
                EntryError::SizeMismatch,
                "claimed size {lie}"
            );
        }
    }

    #[test]
    fn an_object_length_that_does_not_fit_the_tail_is_rejected() {
        let (object, _) = object(b"artifact");
        let len = object.len() as u64;
        // Shorter than the bytes read: not possible for a real suffix.
        for shorter in [0, len - 1] {
            assert_eq!(
                Trailer::parse_tail(&object, shorter).unwrap_err(),
                EntryError::BadTrailerLength,
                "object length {shorter}"
            );
        }
        // Longer than the bytes read while the trailer says otherwise.
        for longer in [len + 1, u64::MAX] {
            assert_eq!(
                Trailer::parse_tail(&object, longer).unwrap_err(),
                EntryError::SizeMismatch,
                "object length {longer}"
            );
        }
        // A size that overflows when the trailer and footer are added.
        let mut huge = trailer(u64::MAX);
        let bytes = huge.encode().unwrap();
        assert_eq!(
            Trailer::parse_tail(&bytes, bytes.len() as u64).unwrap_err(),
            EntryError::SizeMismatch
        );
        huge.size = u64::MAX - 5;
        let bytes = huge.encode().unwrap();
        assert_eq!(
            Trailer::parse_tail(&bytes, u64::MAX).unwrap_err(),
            EntryError::SizeMismatch
        );
    }

    #[test]
    fn unsupported_versions_and_bad_hashes_are_rejected() {
        let mut bad_version = trailer(0);
        bad_version.version = 2;
        // `encode` refuses to write it, so build the bytes directly.
        let mut bytes = rmp_serde::to_vec_named(&bad_version).unwrap();
        let length = bytes.len() as u32;
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(MAGIC);
        assert_eq!(
            Trailer::parse_tail(&bytes, bytes.len() as u64).unwrap_err(),
            EntryError::UnsupportedVersion
        );
        assert_eq!(bad_version.encode().unwrap_err(), EntryError::Malformed);

        for hash in [
            "",
            "A".repeat(64).as_str(),
            "g".repeat(64).as_str(),
            "a".repeat(63).as_str(),
        ] {
            let mut t = trailer(0);
            t.sha256 = hash.into();
            assert_eq!(
                t.encode().unwrap_err(),
                EntryError::Malformed,
                "{hash:?}"
            );
            let mut bytes = rmp_serde::to_vec_named(&t).unwrap();
            let length = bytes.len() as u32;
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(MAGIC);
            assert_eq!(
                Trailer::parse_tail(&bytes, bytes.len() as u64).unwrap_err(),
                EntryError::Malformed,
                "{hash:?}"
            );
        }
    }

    #[test]
    fn a_trailer_over_the_limit_is_not_written() {
        let mut t = trailer(0);
        t.writer_principal = "p".repeat(MAX_TRAILER_LEN);
        assert_eq!(t.encode().unwrap_err(), EntryError::TrailerTooLarge);
        t.writer_principal = "p".repeat(MAX_TRAILER_LEN - 400);
        assert!(t.encode().is_ok());
    }

    /// Builds an object around raw trailer bytes with a footer that names them.
    fn raw_object(artifact: &[u8], trailer_bytes: &[u8]) -> Vec<u8> {
        let mut object = artifact.to_vec();
        object.extend_from_slice(trailer_bytes);
        object.extend_from_slice(&(trailer_bytes.len() as u32).to_be_bytes());
        object.extend_from_slice(MAGIC);
        object
    }

    #[test]
    fn bytes_after_the_trailer_inside_the_region_are_rejected() {
        let encoded = trailer(3).encode().unwrap();
        let mut bytes = encoded[..encoded.len() - FOOTER_LEN].to_vec();
        bytes.push(0xc0); // msgpack nil
        let object = raw_object(b"abc", &bytes);
        assert_eq!(
            Trailer::parse_tail(&object, object.len() as u64).unwrap_err(),
            EntryError::Malformed
        );
    }

    #[test]
    fn a_tail_must_be_the_end_of_the_object() {
        let (object, _) = object(b"artifact");
        assert_eq!(
            Trailer::parse_tail(&object, object.len() as u64 - 1).unwrap_err(),
            EntryError::BadTrailerLength,
            "the tail cannot be longer than the object"
        );
        // A shorter suffix that still holds the whole trailer is enough.
        let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
        let suffix = &object[object.len() - parsed.sealed_len()..];
        assert!(Trailer::parse_tail(suffix, object.len() as u64).is_ok());
        // One byte less cuts the trailer off.
        let cut = &suffix[1..];
        assert!(Trailer::parse_tail(cut, object.len() as u64).is_err());
    }

    #[test]
    fn a_footer_alone_cannot_hold_a_trailer() {
        let mut tail = 5u32.to_be_bytes().to_vec();
        tail.extend_from_slice(MAGIC);
        assert_eq!(tail.len(), FOOTER_LEN);
        assert_eq!(
            Trailer::parse_tail(&tail, 99).unwrap_err(),
            EntryError::BadTrailerLength
        );
    }

    #[test]
    fn the_trailer_limit_is_inclusive_on_write_and_on_read() {
        // Find a principal length that makes the encoding exactly the maximum.
        let mut t = trailer(0);
        t.writer_principal = String::new();
        let base = rmp_serde::to_vec_named(&t).unwrap().len();
        // The string header grows from 1 to 3 bytes once the text passes 31.
        let pad = MAX_TRAILER_LEN - base - 2;
        t.writer_principal = "p".repeat(pad);
        let exact = rmp_serde::to_vec_named(&t).unwrap();
        assert_eq!(exact.len(), MAX_TRAILER_LEN);
        let encoded = t.encode().unwrap();
        assert!(Trailer::parse_tail(&encoded, encoded.len() as u64).is_ok());

        t.writer_principal = "p".repeat(pad + 1);
        assert_eq!(t.encode().unwrap_err(), EntryError::TrailerTooLarge);
        let over = rmp_serde::to_vec_named(&t).unwrap();
        let object = raw_object(b"", &over);
        assert_eq!(
            Trailer::parse_tail(&object, object.len() as u64).unwrap_err(),
            EntryError::BadTrailerLength
        );
    }

    #[test]
    fn a_stored_at_that_is_not_rfc3339_is_malformed() {
        #[derive(Serialize)]
        struct Bad {
            version: u32,
            writer_credential: TrailerCredential,
            writer_principal: &'static str,
            namespace: &'static str,
            stored_at: &'static str,
            size: u64,
            sha256: String,
        }
        for stored_at in ["yesterday", "2026-13-45T00:00:00Z", ""] {
            let bytes = rmp_serde::to_vec_named(&Bad {
                version: TRAILER_VERSION,
                writer_credential: TrailerCredential {
                    kind: "k".into(),
                    id: "i".into(),
                },
                writer_principal: "p",
                namespace: "shared",
                stored_at,
                size: 0,
                sha256: "a".repeat(64),
            })
            .unwrap();
            let object = raw_object(b"", &bytes);
            assert_eq!(
                Trailer::parse_tail(&object, object.len() as u64).unwrap_err(),
                EntryError::Malformed,
                "{stored_at:?}"
            );
        }
    }

    #[test]
    fn the_held_back_check_catches_any_other_object() {
        let (first, _) = object(b"artifact");
        let parsed = Trailer::parse_tail(&first, first.len() as u64).unwrap();
        let held = &first[first.len() - parsed.sealed_len()..];
        assert!(parsed.matches_tail(&first, held));
        // The same artifact stored again at a later time has a different trailer.
        let mut later = trailer(8);
        later.stored_at =
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1);
        let mut second = b"artifact".to_vec();
        second.extend(later.encode().unwrap());
        let second_held =
            &second[second.len() - later.encode().unwrap().len()..];
        assert!(!parsed.matches_tail(&first, second_held));
        assert!(!parsed.matches_tail(&first, &held[1..]), "short");
        assert!(!parsed.matches_tail(&first, &[held, &[0]].concat()), "long");
        assert!(!parsed.matches_tail(&held[1..], held), "tail too short");
    }

    #[test]
    fn hex_helpers() {
        assert_eq!(to_hex(&[0, 15, 255]), "000fff");
        assert!(is_sha256_hex(&"0".repeat(64)));
        for bad in [
            "",
            "A".repeat(64).as_str(),
            "0".repeat(65).as_str(),
            " ".repeat(64).as_str(),
        ] {
            assert!(!is_sha256_hex(bad), "{bad:?}");
        }
    }
}
