//! Local cache partitions and the trust epoch (RFC 0030, section 4.4).
//!
//! A remote-enabled run keeps every local entry in one partition of the cache
//! directory, chosen by how far it trusts what it restores, so entries built on
//! remote content never reach a run that trusts more:
//!
//! ```text
//! <cache_dir>/.partitions/trusted/{epoch}/
//! <cache_dir>/.partitions/untrusted/{epoch}/{principal}/{scope}/
//! <cache_dir>/.partitions/local/
//! ```
//!
//! The directory is dot-prefixed so project directories (base58 names) can
//! never collide with it, and the stats and prune walks skip it as internal.

use std::path::{Component, Path, PathBuf};

use omni_path_utils::segment;
use sha2::{Digest as _, Sha256};

pub(crate) const PARTITIONS_DIR: &str = ".partitions";

/// What goes into a trust epoch. Any change starts a fresh `trusted/`
/// partition, so nothing derived under the old policy is reused.
pub(crate) struct EpochInputs<'a> {
    pub api_base_url: &'a str,
    pub tenant: &'a str,
    pub org: &'a str,
    pub ws: &'a str,
    /// `None` and an empty string are different values.
    pub env: Option<&'a str>,
    /// Effective trusted key ids (slice 7; empty until then).
    pub trusted_key_ids: &'a [String],
    /// The effective verify mode (`off` until slice 7).
    pub verify_mode: &'a str,
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    match trimmed.split_once("://") {
        Some((scheme, rest)) => {
            let (host, path) = match rest.find('/') {
                Some(i) => rest.split_at(i),
                None => (rest, ""),
            };
            format!(
                "{}://{}{path}",
                scheme.to_ascii_lowercase(),
                host.to_ascii_lowercase()
            )
        }
        None => trimmed.to_owned(),
    }
}

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u32).to_be_bytes());
    hash.update(bytes);
}

/// First 16 hex characters of a SHA-256 over a versioned, length-prefixed
/// encoding of the inputs.
pub(crate) fn trust_epoch(inputs: &EpochInputs<'_>) -> String {
    use std::fmt::Write as _;
    let mut hash = Sha256::new();
    field(&mut hash, b"omni-trust-epoch-v1");
    field(&mut hash, normalize_url(inputs.api_base_url).as_bytes());
    field(&mut hash, inputs.tenant.as_bytes());
    field(&mut hash, inputs.org.as_bytes());
    field(&mut hash, inputs.ws.as_bytes());
    match inputs.env {
        Some(env) => {
            field(&mut hash, b"some");
            field(&mut hash, env.as_bytes());
        }
        None => field(&mut hash, b"none"),
    }
    let mut ids: Vec<&String> = inputs.trusted_key_ids.iter().collect();
    ids.sort();
    hash.update((ids.len() as u32).to_be_bytes());
    for id in ids {
        field(&mut hash, id.as_bytes());
    }
    field(&mut hash, inputs.verify_mode.as_bytes());
    Sha256::digest(hash.finalize()).iter().take(8).fold(
        String::new(),
        |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        },
    )
}

/// The directory of the trusted partition for `epoch`.
pub(crate) fn trusted_dir(base: &Path, epoch: &str) -> PathBuf {
    base.join(PARTITIONS_DIR).join("trusted").join(epoch)
}

/// The directory of an untrusted run's partition. `principal` and `scope` are
/// the server's text; they only ever become path segments through
/// [`segment`], so no input can name another directory.
pub(crate) fn untrusted_dir(
    base: &Path,
    epoch: &str,
    principal: &str,
    scope: &str,
) -> PathBuf {
    let dir = base
        .join(PARTITIONS_DIR)
        .join("untrusted")
        .join(epoch)
        .join(segment(principal))
        .join(segment(scope));
    debug_assert!(is_confined(base, &dir));
    dir
}

/// The directory of a run that is known to be untrusted but whose principal and
/// scope the server did not report. No other run reads it, so such a run never
/// writes into a partition a trusted run restores from.
pub(crate) fn unknown_dir(base: &Path, epoch: &str) -> PathBuf {
    base.join(PARTITIONS_DIR).join("unknown").join(epoch)
}

/// The directory for tasks that must never touch the remote cache.
pub(crate) fn local_dir(base: &Path) -> PathBuf {
    base.join(PARTITIONS_DIR).join("local")
}

/// True when `path` is `base` followed only by normal components.
pub(crate) fn is_confined(base: &Path, path: &Path) -> bool {
    path.strip_prefix(base).is_ok_and(|rest| {
        rest.components().all(|c| matches!(c, Component::Normal(_)))
    })
}

/// Every partition directory under `base`: `trusted/*`, `untrusted/*/*/*` and
/// `local`. Unreadable directories are skipped.
pub(crate) async fn existing_partitions(base: &Path) -> Vec<PathBuf> {
    async fn children(dir: &Path) -> Vec<PathBuf> {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.is_ok_and(|t| t.is_dir()) {
                out.push(entry.path());
            }
        }
        out
    }

    let root = base.join(PARTITIONS_DIR);
    let mut found = Vec::new();
    found.extend(children(&root.join("trusted")).await);
    found.extend(children(&root.join("unknown")).await);
    for epoch in children(&root.join("untrusted")).await {
        for principal in children(&epoch).await {
            found.extend(children(&principal).await);
        }
    }
    if tokio::fs::try_exists(root.join("local"))
        .await
        .unwrap_or(false)
    {
        found.push(root.join("local"));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>() -> EpochInputs<'a> {
        EpochInputs {
            api_base_url: "https://cache.example.com/api",
            tenant: "t",
            org: "o",
            ws: "w",
            env: Some("e"),
            trusted_key_ids: &[],
            verify_mode: "off",
        }
    }

    #[test]
    fn the_epoch_is_16_hex_and_stable() {
        let epoch = trust_epoch(&inputs());
        assert_eq!(epoch.len(), 16);
        assert!(epoch.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(epoch, trust_epoch(&inputs()));
    }

    #[test]
    fn every_input_changes_the_epoch() {
        let base = trust_epoch(&inputs());
        let keys = ["b".to_string(), "a".to_string()];
        let variants = [
            EpochInputs {
                api_base_url: "https://other.example.com/api",
                ..inputs()
            },
            EpochInputs {
                tenant: "t2",
                ..inputs()
            },
            EpochInputs {
                org: "o2",
                ..inputs()
            },
            EpochInputs {
                ws: "w2",
                ..inputs()
            },
            EpochInputs {
                env: Some("e2"),
                ..inputs()
            },
            EpochInputs {
                env: None,
                ..inputs()
            },
            EpochInputs {
                env: Some(""),
                ..inputs()
            },
            EpochInputs {
                trusted_key_ids: &keys[..1],
                ..inputs()
            },
            EpochInputs {
                verify_mode: "require",
                ..inputs()
            },
        ];
        let mut seen = std::collections::HashSet::from([base]);
        for variant in &variants {
            assert!(seen.insert(trust_epoch(variant)), "a collision");
        }
    }

    #[test]
    fn key_order_and_url_spelling_do_not_matter() {
        let ab = ["a".to_string(), "b".to_string()];
        let ba = ["b".to_string(), "a".to_string()];
        assert_eq!(
            trust_epoch(&EpochInputs {
                trusted_key_ids: &ab,
                ..inputs()
            }),
            trust_epoch(&EpochInputs {
                trusted_key_ids: &ba,
                ..inputs()
            })
        );
        assert_eq!(
            trust_epoch(&EpochInputs {
                api_base_url: "HTTPS://Cache.Example.com/api/",
                ..inputs()
            }),
            trust_epoch(&inputs())
        );
    }

    #[test]
    fn fields_cannot_shift_into_each_other() {
        // Length prefixes keep ("ab", "c") apart from ("a", "bc").
        assert_ne!(
            trust_epoch(&EpochInputs {
                tenant: "ab",
                org: "c",
                ..inputs()
            }),
            trust_epoch(&EpochInputs {
                tenant: "a",
                org: "bc",
                ..inputs()
            })
        );
    }

    #[test]
    fn hostile_principal_and_scope_text_stays_inside_the_partition_directory() {
        let base = Path::new("/cache");
        for hostile in [
            "..", "../..", "a/b", "%2e%2e", "con", "a\0b", "C:\\x", "", ".",
        ] {
            let dir = untrusted_dir(base, "0123456789abcdef", hostile, hostile);
            assert!(is_confined(base, &dir), "{hostile:?} -> {dir:?}");
            assert!(
                dir.starts_with(
                    "/cache/.partitions/untrusted/0123456789abcdef"
                )
            );
            assert_eq!(dir.components().count(), 7, "{dir:?}");
        }
        assert_eq!(
            untrusted_dir(base, "e", "ci", "pr-1"),
            Path::new("/cache/.partitions/untrusted/e/ci/pr-1")
        );
        assert!(!is_confined(base, Path::new("/cache/../x")));
        assert!(!is_confined(base, Path::new("/other/x")));
    }

    #[tokio::test]
    async fn existing_partitions_lists_each_class() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        for dir in [
            trusted_dir(base, "e1"),
            untrusted_dir(base, "e1", "p", "s"),
            untrusted_dir(base, "e1", "p", "t"),
            unknown_dir(base, "e1"),
            local_dir(base),
        ] {
            tokio::fs::create_dir_all(dir).await.unwrap();
        }
        tokio::fs::create_dir_all(base.join("someproject/output"))
            .await
            .unwrap();
        let mut found = existing_partitions(base).await;
        found.sort();
        assert_eq!(found.len(), 5, "{found:?}");
        assert!(
            found
                .iter()
                .all(|p| p.starts_with(base.join(PARTITIONS_DIR)))
        );
    }
}
