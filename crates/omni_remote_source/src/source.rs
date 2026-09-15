use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

/// An engine-neutral description of a remote source to materialize. Each variant
/// owns its own addressing scheme; the store lays every kind out under
/// `store/<kind>/<key>/`, where the key is an immutable post-fetch content
/// identifier.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteSource {
    Git { uri: Url, rev: String },
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
