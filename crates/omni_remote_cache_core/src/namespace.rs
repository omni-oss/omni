//! Where an entry lives: the namespace part of a storage container (RFC 0030,
//! section 4.1).
//!
//! ```text
//! shared/{tenant}/{org}/{ws}/{env}
//! isolated/{tenant}/{org}/{ws}/{env}/{principal_id}/{scope}
//! ```
//!
//! The first segment is always `shared` or `isolated`, so a bucket lifecycle rule
//! can match one prefix, and an escaped resource code can never name another
//! namespace. Only this module builds the container of an artifact route, from
//! the authenticated caller and a validated request, so no client-supplied text
//! reaches a path unchecked.

use omni_path_utils::is_safe_literal;
pub use omni_path_utils::{MAX_SCOPE_LEN, is_valid_scope_label, segment};
use omni_remote_cache_storage_core::{Container, ContainerPrefix};

use crate::{
    hooks::NamespaceKind, routes::v1::artifacts::common::MAX_DIGEST_LEN,
    security::valid_identity_text, utils::path::escape_path_component,
};

/// The longest escaped resource code (`tenant`, `org`, `ws`, `env`), in bytes.
/// Longer codes are refused instead of failing later inside a backend.
pub const MAX_CODE_LEN: usize = 255;

/// The longest object key any backend accepts (S3's limit). The key is the
/// container, a `/` and the digest.
pub const MAX_KEY_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamespaceError {
    #[error("a resource code is longer than {MAX_CODE_LEN} bytes once escaped")]
    CodeTooLong,
    #[error("the storage key would be longer than {MAX_KEY_LEN} bytes")]
    KeyTooLong,
    #[error("the principal is not valid identity text")]
    InvalidPrincipal,
    #[error(
        "a scope label must be 1 to {MAX_SCOPE_LEN} characters of letters, digits, '.', '_' and '-', starting with a letter or digit"
    )]
    InvalidScope,
    #[error("the container is not valid")]
    InvalidContainer,
}

/// A scope label that passed [`is_valid_scope_label`], so code that builds a
/// namespace cannot be handed an unchecked one. A caller's `scope_pin` and the
/// request's `scope` both become one before they are used.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeLabel(String);

impl ScopeLabel {
    pub fn parse(label: &str) -> Result<Self, NamespaceError> {
        if is_valid_scope_label(label) {
            Ok(Self(label.to_owned()))
        } else {
            Err(NamespaceError::InvalidScope)
        }
    }

    /// The label used when neither a pin nor a request value names one.
    pub fn default_label() -> Self {
        Self("default".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which part of the cache an entry belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Entries a publisher stored for everyone who may read the environment.
    Shared,
    /// Entries one principal stored for itself, split by scope label.
    Isolated {
        /// The path segment for the principal (see [`segment`]).
        principal_id: String,
        /// The path segment for the scope label.
        scope_id: String,
    },
}

impl Namespace {
    /// The isolated namespace of `principal` for `scope`.
    ///
    /// `principal` comes only from the authenticated caller, and `scope` is the
    /// caller's pin or the validated request label; neither is ever used as a
    /// path directly.
    pub fn isolated(
        principal: &str,
        scope: &ScopeLabel,
    ) -> Result<Self, NamespaceError> {
        if !valid_identity_text(principal) {
            return Err(NamespaceError::InvalidPrincipal);
        }
        Ok(Self::Isolated {
            principal_id: segment(principal),
            scope_id: segment(scope.as_str()),
        })
    }

    pub fn kind(&self) -> NamespaceKind {
        match self {
            Self::Shared => NamespaceKind::Shared,
            Self::Isolated { .. } => NamespaceKind::Isolated,
        }
    }

    /// The container of one environment in this namespace.
    pub fn container(
        &self,
        tenant: &str,
        org: &str,
        ws: &str,
        env: &str,
    ) -> Result<Container, NamespaceError> {
        let mut path = self.env_path(tenant, org, ws, env)?;
        if let Self::Isolated {
            principal_id,
            scope_id,
        } = self
        {
            path.push('/');
            path.push_str(principal_id);
            path.push('/');
            path.push_str(scope_id);
        }
        if path.len() + 1 + MAX_DIGEST_LEN > MAX_KEY_LEN {
            return Err(NamespaceError::KeyTooLong);
        }
        Container::parse(path).map_err(|_| NamespaceError::InvalidContainer)
    }

    /// What a list covers: the shared environment, or for an isolated namespace
    /// the whole principal (every scope of it).
    pub fn list_prefix(
        &self,
        tenant: &str,
        org: &str,
        ws: &str,
        env: &str,
    ) -> Result<ContainerPrefix, NamespaceError> {
        match self {
            Self::Shared => Ok(self.container(tenant, org, ws, env)?.prefix()),
            Self::Isolated { principal_id, .. } => {
                let path = format!(
                    "{}/{principal_id}/",
                    self.env_path(tenant, org, ws, env)?
                );
                ContainerPrefix::parse(path)
                    .map_err(|_| NamespaceError::InvalidContainer)
            }
        }
    }

    fn env_path(
        &self,
        tenant: &str,
        org: &str,
        ws: &str,
        env: &str,
    ) -> Result<String, NamespaceError> {
        let mut path = String::from(match self {
            Self::Shared => "shared",
            Self::Isolated { .. } => "isolated",
        });
        for code in [tenant, org, ws, env] {
            let escaped = escape_path_component(code);
            if escaped.len() > MAX_CODE_LEN {
                return Err(NamespaceError::CodeTooLong);
            }
            path.push('/');
            path.push_str(&escaped);
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(ns: &Namespace, codes: [&str; 4]) -> String {
        ns.container(codes[0], codes[1], codes[2], codes[3])
            .unwrap()
            .to_string()
    }

    const CODES: [&str; 4] = ["t", "o", "w", "e"];

    fn iso(principal: &str, scope: &str) -> Result<Namespace, NamespaceError> {
        Namespace::isolated(principal, &ScopeLabel::parse(scope)?)
    }

    #[test]
    fn the_namespace_is_always_the_first_segment() {
        assert_eq!(container(&Namespace::Shared, CODES), "shared/t/o/w/e");
        let ns = iso("ci", "pr-1").unwrap();
        assert_eq!(container(&ns, CODES), "isolated/t/o/w/e/ci/pr-1");
        assert_eq!(ns.kind(), NamespaceKind::Isolated);
        assert_eq!(Namespace::Shared.kind(), NamespaceKind::Shared);
    }

    #[test]
    fn safe_literals_are_lowercase_bounded_and_not_device_names() {
        for ok in [
            "a",
            "ci",
            "pr-12",
            "a.b_c-d",
            "0",
            "x".repeat(64).as_str(),
            "h",
            "h_x",
            "hx-y",
        ] {
            assert!(is_safe_literal(ok), "{ok:?}");
        }
        for bad in [
            "",
            "Ab",
            "AB",
            "aB",
            "a.",
            "a..",
            ".a",
            "-a",
            "_a",
            "a b",
            "a/b",
            "a\\b",
            "a:b",
            "é",
            "a\u{0}",
            "h-",
            "h-abc",
            "h-0123456789abcdef0123456789abcdef",
            "con",
            "nul",
            "prn",
            "aux",
            "com1",
            "lpt9",
            "con.txt",
            "nul.a.b",
            "com5.log",
            "x".repeat(65).as_str(),
        ] {
            assert!(!is_safe_literal(bad), "{bad:?}");
        }
        // Not device names: a longer stem, or a digit outside 1-9.
        for ok in ["console", "com0", "com10", "lpt", "auxiliary", "nulls"] {
            assert!(is_safe_literal(ok), "{ok:?}");
        }
    }

    #[test]
    fn anything_that_is_not_a_safe_literal_is_hashed() {
        let hashed = segment("Ab");
        assert!(hashed.starts_with("h-"));
        assert_eq!(hashed.len(), 34);
        assert!(
            hashed[2..]
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert_eq!(hashed, segment("Ab"), "stable");
        assert_eq!(segment("ci"), "ci", "a safe literal is kept");
        for text in [
            "repo:org/name:ref:refs/heads/x",
            "é",
            "CON",
            "a.",
            "x".repeat(65).as_str(),
            "has space",
            "a\\b",
        ] {
            let s = segment(text);
            assert!(s.starts_with("h-") && s.len() == 34, "{text:?} -> {s:?}");
            assert!(!s.contains('/') && !s.contains('\\') && !s.contains(':'));
        }
    }

    #[test]
    fn every_segment_is_its_own_lowercase_form_so_case_can_never_alias() {
        // The property a case-insensitive file system needs: no segment has an
        // uppercase letter, and different inputs still give different segments.
        let mut seen = std::collections::HashMap::new();
        for text in [
            "ab",
            "Ab",
            "aB",
            "AB",
            "a.",
            "A.",
            "con",
            "CON",
            "Con",
            "nul.txt",
            "NUL.TXT",
            "x",
            "X",
            "pr-1",
            "PR-1",
            "Release_1.2",
        ] {
            let s = segment(text);
            assert_eq!(s, s.to_lowercase(), "{text:?} -> {s:?}");
            if let Some(other) = seen.insert(s.clone(), text) {
                panic!("{text:?} and {other:?} share {s:?}");
            }
        }
        assert!(
            segment("X").starts_with("h-"),
            "an uppercase label is hashed"
        );
        assert_eq!(segment("x"), "x");
    }

    #[test]
    fn a_literal_that_looks_hashed_is_hashed_so_it_cannot_imitate_another_principal()
     {
        let victim = segment("Victim Principal");
        assert!(victim.starts_with("h-"));
        // An attacker whose principal is exactly the victim's segment text.
        let imitation = segment(&victim);
        assert_ne!(imitation, victim, "it is hashed again, not taken as is");
        assert!(!is_safe_literal(&victim));
        for text in
            [victim.as_str(), "H-0123456789ABCDEF0123456789ABCDEF", "h-x"]
        {
            assert!(segment(text).starts_with("h-") && segment(text) != text);
        }
    }

    #[test]
    fn principals_that_share_a_long_prefix_differ() {
        let long = "p".repeat(200);
        let a = format!("{long}a");
        let b = format!("{long}b");
        assert_ne!(segment(&a), segment(&b));
    }

    #[test]
    fn scope_labels_follow_the_rfc_grammar() {
        for ok in [
            "a",
            "A",
            "0",
            "pr-12",
            "Release_1.2",
            "x".repeat(64).as_str(),
        ] {
            assert!(is_valid_scope_label(ok), "{ok:?}");
            assert!(iso("p", ok).is_ok(), "{ok:?}");
        }
        for bad in [
            "",
            ".",
            "..",
            "-a",
            "_a",
            ".a",
            "a/b",
            "../x",
            "a b",
            "é",
            "a:b",
            "a\\b",
            "x".repeat(65).as_str(),
        ] {
            assert!(!is_valid_scope_label(bad), "{bad:?}");
            assert_eq!(
                iso("p", bad).unwrap_err(),
                NamespaceError::InvalidScope,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_principal_is_any_bounded_text_and_never_a_path() {
        for ok in [
            "owner",
            "Owner",
            "repo:org/name:ref:refs/heads/x",
            "é",
            "../../x",
            "shared",
        ] {
            let ns = iso(ok, "default").unwrap();
            let path = container(&ns, CODES);
            assert!(path.starts_with("isolated/t/o/w/e/"), "{path}");
            assert_eq!(
                path.matches('/').count(),
                6,
                "exactly two more segments: {path}"
            );
            assert!(!path.contains(".."), "{path}");
        }
        for bad in ["", "a\nb", "a\u{0}b", "x".repeat(257).as_str()] {
            assert_eq!(
                iso(bad, "default").unwrap_err(),
                NamespaceError::InvalidPrincipal,
                "{bad:?}"
            );
        }
        assert!(iso(&"x".repeat(256), "default").is_ok());
    }

    #[test]
    fn resource_codes_cannot_add_segments_or_name_another_namespace() {
        let shared = Namespace::Shared;
        for hostile in [
            "a/b", "..", ".", "../..", "shared", "isolated", "a\\b", "%2F",
            "x%", "a.", "con",
        ] {
            let path = container(&shared, [hostile, "o", "w", "e"]);
            assert!(path.starts_with("shared/"), "{path}");
            assert_eq!(path.matches('/').count(), 4, "{hostile:?} -> {path}");
            assert!(!path.split('/').any(|s| s == ".." || s == "."), "{path}");
        }
        // Codes that look like a namespace stay below their own.
        assert_eq!(
            container(&shared, ["isolated", "o", "w", "e"]),
            "shared/isolated/o/w/e"
        );
        let iso = iso("p", "s").unwrap();
        assert_eq!(
            container(&iso, ["shared", "o", "w", "e"]),
            "isolated/shared/o/w/e/p/s"
        );
    }

    #[test]
    fn distinct_inputs_never_share_a_container() {
        let mut all = std::collections::HashMap::new();
        let mut put = |label: String, c: String| {
            if let Some(prev) = all.insert(c.clone(), label.clone()) {
                panic!("{label} and {prev} share {c}");
            }
        };
        for shared_codes in [
            ["t", "o", "w", "e"],
            ["t/o", "w", "e", "x"],
            ["t", "o/w", "e", "x"],
            ["", "t", "o", "w"],
        ] {
            put(
                format!("shared {shared_codes:?}"),
                container(&Namespace::Shared, shared_codes),
            );
        }
        for (principal, scope) in [
            ("a", "x"),
            ("a", "y"),
            ("b", "x"),
            ("A", "x"),
            ("a", "X"),
            ("h-a", "x"),
            ("a.", "x"),
            ("é", "x"),
        ] {
            let ns = iso(principal, scope).unwrap();
            put(
                format!("isolated {principal}/{scope}"),
                container(&ns, CODES),
            );
        }
        // Shared and isolated can never collide: the first segment differs.
        assert!(all.keys().all(|c| c.starts_with("shared/") || c.starts_with("isolated/")));
    }

    #[test]
    fn two_principals_have_disjoint_prefixes() {
        let a = iso("ci", "x").unwrap();
        let b = iso("ci2", "x").unwrap();
        let pa = a.list_prefix("t", "o", "w", "e").unwrap();
        let pb = b.list_prefix("t", "o", "w", "e").unwrap();
        assert_eq!(pa.as_str(), "isolated/t/o/w/e/ci/");
        assert_eq!(pb.as_str(), "isolated/t/o/w/e/ci2/");
        assert!(
            !pb.as_str().starts_with(pa.as_str()),
            "ci must not match ci2"
        );
        let own = a.container("t", "o", "w", "e").unwrap();
        assert!(own.as_str().starts_with(pa.as_str()));
        assert!(!own.as_str().starts_with(pb.as_str()));
        // A list of the shared environment is that container's prefix.
        assert_eq!(
            Namespace::Shared
                .list_prefix("t", "o", "w", "e")
                .unwrap()
                .as_str(),
            "shared/t/o/w/e/"
        );
        // The isolated list covers every scope of the principal.
        let other_scope = iso("ci", "y")
            .unwrap()
            .container("t", "o", "w", "e")
            .unwrap();
        assert!(other_scope.as_str().starts_with(pa.as_str()));
    }

    /// Codes of these lengths (all `a`), for budget arithmetic.
    fn codes(lens: [usize; 4]) -> [String; 4] {
        lens.map(|n| "a".repeat(n))
    }

    fn shared(c: &[String; 4]) -> Result<Container, NamespaceError> {
        Namespace::Shared.container(&c[0], &c[1], &c[2], &c[3])
    }

    #[test]
    fn a_code_is_limited_after_escaping() {
        let ns = Namespace::Shared;
        assert!(ns.container(&"a".repeat(255), "o", "w", "e").is_ok());
        assert_eq!(
            ns.container(&"a".repeat(256), "o", "w", "e").unwrap_err(),
            NamespaceError::CodeTooLong
        );
        // Escaping counts: '/' becomes three bytes.
        assert!(ns.container(&"/".repeat(85), "o", "w", "e").is_ok());
        assert_eq!(
            ns.container(&"/".repeat(86), "o", "w", "e").unwrap_err(),
            NamespaceError::CodeTooLong,
            "86 slashes escape to 258 bytes"
        );
        // Every code and every prefix builder is held to the same limit.
        for position in 0..4 {
            let mut c = ["t", "o", "w", "e"].map(String::from);
            c[position] = "a".repeat(256);
            assert_eq!(shared(&c).unwrap_err(), NamespaceError::CodeTooLong);
            assert_eq!(
                Namespace::Shared
                    .list_prefix(&c[0], &c[1], &c[2], &c[3])
                    .unwrap_err(),
                NamespaceError::CodeTooLong
            );
            assert_eq!(
                iso("p", "s")
                    .unwrap()
                    .list_prefix(&c[0], &c[1], &c[2], &c[3])
                    .unwrap_err(),
                NamespaceError::CodeTooLong
            );
        }
    }

    #[test]
    fn the_key_budget_is_exact() {
        // A shared container is 6 + 4 + (sum of the code lengths) bytes, and the
        // key adds '/' and a 64 byte digest: it must stay within 1024.
        let fits = codes([255, 255, 255, 184]);
        let container = shared(&fits).unwrap();
        assert_eq!(container.as_str().len() + 1 + MAX_DIGEST_LEN, MAX_KEY_LEN);
        assert_eq!(
            shared(&codes([255, 255, 255, 185])).unwrap_err(),
            NamespaceError::KeyTooLong,
            "one byte more"
        );
    }

    #[test]
    fn the_isolated_segments_count_toward_the_budget_too() {
        // Fine as a shared container, too long once the principal and scope
        // segments (64 characters each) and the longer namespace name are added.
        let c = codes([255, 255, 255, 65]);
        assert!(shared(&c).is_ok());
        let long = iso(&"p".repeat(64), &"s".repeat(64)).unwrap();
        assert_eq!(
            long.container(&c[0], &c[1], &c[2], &c[3]).unwrap_err(),
            NamespaceError::KeyTooLong
        );
        // The same codes fit when the segments are short.
        let short = iso("p", "s").unwrap();
        assert!(short.container(&c[0], &c[1], &c[2], &c[3]).is_ok());
    }

    #[test]
    fn principal_length_is_counted_in_bytes_and_long_ones_are_hashed() {
        let literal = "p".repeat(64);
        assert_eq!(segment(&literal), literal, "64 characters stay literal");
        assert!(segment(&"p".repeat(65)).starts_with("h-"), "65 are hashed");
        assert!(iso(&"x".repeat(256), "s").is_ok());
        assert_eq!(
            iso(&"x".repeat(257), "s").unwrap_err(),
            NamespaceError::InvalidPrincipal
        );
        // 128 two-byte characters is 256 bytes.
        assert!(iso(&"é".repeat(128), "s").is_ok());
        assert_eq!(
            iso(&"é".repeat(129), "s").unwrap_err(),
            NamespaceError::InvalidPrincipal
        );
    }

    #[test]
    fn unicode_forms_of_one_name_are_separate_principals() {
        let nfc = "\u{e9}";
        let nfd = "e\u{301}";
        assert_ne!(nfc, nfd);
        assert_ne!(segment(nfc), segment(nfd));
    }

    #[test]
    fn a_principal_that_spells_a_hashed_segment_is_still_hashed() {
        let victim = iso("Some Principal", "s").unwrap();
        let Namespace::Isolated {
            principal_id: hashed,
            ..
        } = &victim
        else {
            unreachable!()
        };
        for spelled in [
            hashed.clone(),
            hashed.to_uppercase(),
            "h-".to_string(),
            "H-x".to_string(),
        ] {
            let attacker = iso(&spelled, "s").unwrap();
            assert_ne!(attacker, victim, "{spelled:?}");
            assert_ne!(
                container(&attacker, CODES),
                container(&victim, CODES),
                "{spelled:?}"
            );
        }
    }
}
