//! The origin marker of an entry restored from the remote cache (RFC 0030,
//! section 4.4). Partitions already decide who may read an entry; the marker
//! records where it came from, so a run that must not reuse isolated content
//! can tell, even if a cache directory was shared against the guidance.

use std::path::Path;

use system_traits::{FsReadAsync as _, impls::RealSys};

pub(crate) const ORIGIN_MARKER: &str = ".omni-origin-remote";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Shared,
    Isolated,
}

/// What a marker records: where the entry came from and which trusted key
/// verified it at restore time (never taken from the server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marker {
    pub origin: Origin,
    pub verified_by: Option<String>,
}

impl Marker {
    /// One line: `v1 origin=<shared|isolated> verified_by=<16 hex|none>`.
    pub(crate) fn text(&self) -> String {
        let origin = match self.origin {
            Origin::Shared => "shared",
            Origin::Isolated => "isolated",
        };
        format!(
            "v1 origin={origin} verified_by={}\n",
            self.verified_by.as_deref().unwrap_or("none")
        )
    }

    /// Strict: anything that is not exactly the line above reads as isolated
    /// and unverified, the least trusted reading.
    fn parse(text: &str) -> Self {
        let worst = Self {
            origin: Origin::Isolated,
            verified_by: None,
        };
        let Some(line) = text.strip_suffix('\n') else {
            return worst;
        };
        let words: Vec<&str> = line.split(' ').collect();
        let [version, origin, verified] = words[..] else {
            return worst;
        };
        let origin = match (version, origin) {
            ("v1", "origin=shared") => Origin::Shared,
            ("v1", "origin=isolated") => Origin::Isolated,
            _ => return worst,
        };
        let verified_by = match verified.strip_prefix("verified_by=") {
            Some("none") => None,
            Some(id) if omni_remote_cache_client::is_key_id(id) => {
                Some(id.to_owned())
            }
            _ => return worst,
        };
        // A key id cannot vouch for an isolated entry.
        if origin == Origin::Isolated && verified_by.is_some() {
            return worst;
        }
        Self {
            origin,
            verified_by,
        }
    }
}

/// The marker in `entry_dir`: `None` for an entry with no marker (one built
/// locally), `Some` otherwise.
pub(crate) async fn read_marker(
    sys: &RealSys,
    entry_dir: &Path,
) -> Option<Marker> {
    let worst = || Marker {
        origin: Origin::Isolated,
        verified_by: None,
    };
    match sys.fs_read_async(entry_dir.join(ORIGIN_MARKER)).await {
        Ok(bytes) => {
            // A marker is one short line; anything else is unreadable.
            if bytes.len() > 256 {
                return Some(worst());
            }
            Some(Marker::parse(&String::from_utf8_lossy(&bytes)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(worst()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef";

    fn marker(origin: Origin, verified_by: Option<&str>) -> Marker {
        Marker {
            origin,
            verified_by: verified_by.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn markers_round_trip_and_anything_odd_is_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert_eq!(read_marker(&RealSys, dir).await, None);
        for m in [
            marker(Origin::Shared, None),
            marker(Origin::Shared, Some(KEY)),
            marker(Origin::Isolated, None),
        ] {
            tokio::fs::write(dir.join(ORIGIN_MARKER), m.text())
                .await
                .unwrap();
            assert_eq!(read_marker(&RealSys, dir).await, Some(m));
        }
        let worst = Some(marker(Origin::Isolated, None));
        for odd in [
            "",
            "garbage",
            "v2 origin=shared verified_by=none\n",
            "v1 origin=shared\n",
            "v1 origin=shared verified_by=none",
            "v1 origin=shared verified_by=ABCDEF0123456789\n",
            "v1 origin=shared verified_by=0123\n",
            "v1 origin=isolated verified_by=0123456789abcdef\n",
            "v1 origin=shared verified_by=none extra\n",
            "v1  origin=shared verified_by=none\n",
        ] {
            tokio::fs::write(dir.join(ORIGIN_MARKER), odd)
                .await
                .unwrap();
            assert_eq!(read_marker(&RealSys, dir).await, worst, "{odd:?}");
        }
        tokio::fs::write(dir.join(ORIGIN_MARKER), "x".repeat(1000))
            .await
            .unwrap();
        assert_eq!(read_marker(&RealSys, dir).await, worst);
        // A marker that is a directory cannot be read: isolated.
        tokio::fs::remove_file(dir.join(ORIGIN_MARKER))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.join(ORIGIN_MARKER))
            .await
            .unwrap();
        assert_eq!(read_marker(&RealSys, dir).await, worst);
    }
}
