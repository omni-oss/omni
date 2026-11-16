//! The origin marker of an entry restored from the remote cache (RFC 0030,
//! section 4.4). Partitions already decide who may read an entry; the marker
//! records where it came from, so a run that must not reuse isolated content
//! can tell, even if a cache directory was shared against the guidance.

use std::path::Path;

pub(crate) const ORIGIN_MARKER: &str = ".omni-origin-remote";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Shared,
    Isolated,
}

impl Origin {
    /// One line: `v1 origin=<shared|isolated> verified_by=none`. Signature
    /// verification (a later slice) fills `verified_by`.
    pub(crate) fn marker_text(self) -> String {
        let origin = match self {
            Self::Shared => "shared",
            Self::Isolated => "isolated",
        };
        format!("v1 origin={origin} verified_by=none\n")
    }

    /// Anything that is not exactly a shared marker reads as isolated, the
    /// least trusted origin.
    fn parse(text: &str) -> Self {
        let mut words = text.split_whitespace();
        match (words.next(), words.next()) {
            (Some("v1"), Some("origin=shared")) => Self::Shared,
            _ => Self::Isolated,
        }
    }
}

/// The origin recorded in `entry_dir`: `None` for an entry with no marker (one
/// built locally), `Some` otherwise.
pub(crate) async fn read_marker(entry_dir: &Path) -> Option<Origin> {
    match tokio::fs::read(entry_dir.join(ORIGIN_MARKER)).await {
        Ok(bytes) => {
            // A marker is one short line; anything else is unreadable.
            if bytes.len() > 256 {
                return Some(Origin::Isolated);
            }
            Some(Origin::parse(&String::from_utf8_lossy(&bytes)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(Origin::Isolated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn markers_round_trip_and_anything_odd_is_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert_eq!(read_marker(dir).await, None);
        for origin in [Origin::Shared, Origin::Isolated] {
            tokio::fs::write(dir.join(ORIGIN_MARKER), origin.marker_text())
                .await
                .unwrap();
            assert_eq!(read_marker(dir).await, Some(origin));
        }
        for odd in ["", "garbage", "v2 origin=shared", "origin=shared"] {
            tokio::fs::write(dir.join(ORIGIN_MARKER), odd)
                .await
                .unwrap();
            assert_eq!(
                read_marker(dir).await,
                Some(Origin::Isolated),
                "{odd:?}"
            );
        }
        tokio::fs::write(dir.join(ORIGIN_MARKER), "x".repeat(1000))
            .await
            .unwrap();
        assert_eq!(read_marker(dir).await, Some(Origin::Isolated));
        // A marker that is a directory cannot be read: isolated.
        tokio::fs::remove_file(dir.join(ORIGIN_MARKER))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.join(ORIGIN_MARKER))
            .await
            .unwrap();
        assert_eq!(read_marker(dir).await, Some(Origin::Isolated));
    }
}
