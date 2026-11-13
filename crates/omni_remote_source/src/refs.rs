use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

/// References keyed `kind -> locator -> selector`, the same shape as the
/// lockfile.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RefSetDataV1_0_0 {
    pub sources: BTreeMap<String, BTreeMap<String, BTreeMap<String, PinRef>>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PinRef {
    pub pin: String,
}

impl RefSetDataV1_0_0 {
    pub fn insert(
        &mut self,
        kind: &str,
        locator: &str,
        selector: &str,
        pin: &str,
    ) {
        self.sources
            .entry(kind.to_string())
            .or_default()
            .entry(locator.to_string())
            .or_default()
            .insert(
                selector.to_string(),
                PinRef {
                    pin: pin.to_string(),
                },
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_file_data_serde::{Format, from_slice, to_vec};

    #[test]
    fn ref_set_round_trips_through_json() {
        let mut data = RefSetDataV1_0_0::default();
        data.insert("git", "https://example.com/a.git", "main", "c1");
        let data = RefSetData::V1_0_0(data);

        let bytes = to_vec(&data, Format::Json).unwrap();
        let back: RefSetData = from_slice(&bytes, Format::Json).unwrap();

        assert_eq!(data, back);
    }

    #[test]
    fn ref_set_serializes_sorted_regardless_of_insertion_order() {
        let mut forward = RefSetDataV1_0_0::default();
        forward.insert("git", "https://example.com/a.git", "dev", "c2");
        forward.insert("git", "https://example.com/a.git", "main", "c1");
        forward.insert("git", "https://example.com/b.git", "v1", "c4");
        forward.insert("fake", "x", "s1", "p1");

        let mut reversed = RefSetDataV1_0_0::default();
        reversed.insert("fake", "x", "s1", "p1");
        reversed.insert("git", "https://example.com/b.git", "v1", "c4");
        reversed.insert("git", "https://example.com/a.git", "main", "c1");
        reversed.insert("git", "https://example.com/a.git", "dev", "c2");

        assert_eq!(
            to_vec(&RefSetData::V1_0_0(forward), Format::Json).unwrap(),
            to_vec(&RefSetData::V1_0_0(reversed), Format::Json).unwrap()
        );
    }

    #[test]
    fn old_and_malformed_shapes_fail_to_load() {
        for doc in [
            r#"{"version":"1.0.0","git":{"https://h/r.git":{"main":{"commit":"c1"}}}}"#,
            r#"{"version":"1.0.0","sources":{"git":{"https://h/r.git":{"main":{"pin":"c1","extra":1}}}}}"#,
            r#"{"version":"1.0.0"}"#,
        ] {
            assert!(
                from_slice::<RefSetData>(doc.as_bytes(), Format::Json).is_err(),
                "{doc}"
            );
        }
    }
}
