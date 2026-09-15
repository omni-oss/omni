use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

/// The persisted reference set for a single subsystem, written to
/// `refs/<id>.json`. It records every remote source the subsystem resolved on
/// its last run, together with the pin it resolved to. Garbage collection unions
/// these files to decide what the store must keep.
///
/// Keys are kept in a [`BTreeMap`] so the serialized form is sorted regardless
/// of the order references were added, keeping committed-adjacent diffs stable.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "version", rename_all = "kebab-case")]
pub enum RefSetData {
    #[serde(rename = "1.0.0")]
    V1_0_0(RefSetDataV1_0_0),
}

impl Default for RefSetData {
    fn default() -> Self {
        Self::V1_0_0(RefSetDataV1_0_0::default())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RefSetDataV1_0_0 {
    pub git: BTreeMap<Url, BTreeMap<String, GitRef>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct GitRef {
    pub commit: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_file_data_serde::{Format, from_slice, to_vec};

    fn git_ref(commit: &str) -> GitRef {
        GitRef {
            commit: commit.to_string(),
        }
    }

    #[test]
    fn ref_set_round_trips_through_json() {
        let mut git = BTreeMap::new();
        let mut revs = BTreeMap::new();
        revs.insert("main".to_string(), git_ref("c1"));
        git.insert(Url::parse("https://example.com/a.git").unwrap(), revs);

        let data = RefSetData::V1_0_0(RefSetDataV1_0_0 { git });
        let bytes = to_vec(&data, Format::Json).unwrap();
        let back: RefSetData = from_slice(&bytes, Format::Json).unwrap();

        assert_eq!(data, back);
    }

    #[test]
    fn ref_set_serializes_sorted_regardless_of_insertion_order() {
        let a = Url::parse("https://example.com/a.git").unwrap();
        let b = Url::parse("https://example.com/b.git").unwrap();

        let mut forward = BTreeMap::new();
        forward.insert(a.clone(), {
            let mut m = BTreeMap::new();
            m.insert("dev".to_string(), git_ref("c2"));
            m.insert("main".to_string(), git_ref("c1"));
            m
        });
        forward.insert(b.clone(), {
            let mut m = BTreeMap::new();
            m.insert("v1".to_string(), git_ref("c4"));
            m
        });

        let mut reversed = BTreeMap::new();
        reversed.insert(b, {
            let mut m = BTreeMap::new();
            m.insert("v1".to_string(), git_ref("c4"));
            m
        });
        reversed.insert(a, {
            let mut m = BTreeMap::new();
            m.insert("main".to_string(), git_ref("c1"));
            m.insert("dev".to_string(), git_ref("c2"));
            m
        });

        let forward = to_vec(
            &RefSetData::V1_0_0(RefSetDataV1_0_0 { git: forward }),
            Format::Json,
        )
        .unwrap();
        let reversed = to_vec(
            &RefSetData::V1_0_0(RefSetDataV1_0_0 { git: reversed }),
            Format::Json,
        )
        .unwrap();

        assert_eq!(forward, reversed);
    }
}
