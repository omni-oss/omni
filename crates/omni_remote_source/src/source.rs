use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::driver::{Connection, SourceSpec};

/// An engine-neutral description of a remote source to materialize. Each variant
/// owns its own addressing scheme and maps to a [`SourceSpec`] handled by the
/// driver registered for its kind.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteSource {
    Git { uri: Url, rev: String },
}

impl RemoteSource {
    /// The kind of this source: its store path segment and lockfile key.
    pub fn kind(&self) -> &'static str {
        match self {
            RemoteSource::Git { .. } => crate::git::GIT_KIND,
        }
    }

    /// The kind-neutral address the store core works with.
    pub fn spec(&self) -> SourceSpec {
        match self {
            RemoteSource::Git { uri, rev } => SourceSpec {
                locator: crate::git::canonical_git_locator(uri),
                selector: rev.clone(),
                connection: Connection::new(uri.as_str()),
            },
        }
    }
}

/// A materialized source: its checkout root on disk and the immutable pin that
/// identifies its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSource {
    pub root: PathBuf,
    pub pin: String,
}

/// A resolved reference to a remote source together with the pin it resolved
/// to. Subsystems return these after materializing; the manager persists them
/// as garbage-collection roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSourceRef {
    pub source: RemoteSource,
    pub pin: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_file_data_serde::{Format, from_slice, to_vec};

    #[test]
    fn remote_source_round_trips_through_json() {
        let source = RemoteSource::Git {
            uri: Url::parse("https://example.com/a.git").unwrap(),
            rev: "main".to_string(),
        };

        let bytes = to_vec(&source, Format::Json).unwrap();
        let back: RemoteSource = from_slice(&bytes, Format::Json).unwrap();

        assert_eq!(source, back);
    }

    #[test]
    fn git_variant_is_tagged_kebab_case() {
        let source = RemoteSource::Git {
            uri: Url::parse("https://example.com/a.git").unwrap(),
            rev: "main".to_string(),
        };

        let json =
            String::from_utf8(to_vec(&source, Format::Json).unwrap()).unwrap();

        assert!(json.contains("\"git\""), "unexpected json: {json}");
    }
}
