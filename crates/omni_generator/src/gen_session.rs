use std::{path::Path, sync::Mutex};

use maps::UnorderedMap;
use omni_generator_configurations::OmniPath;
use serde::Serialize;
use system_traits::{FsReadAsync, FsWriteAsync};
use value_bag::{OwnedValueBag, ValueBag};

#[derive(Debug, Default)]
pub struct GenSession {
    data: Mutex<UnorderedMap<String, DataImpl>>,
}

impl GenSession {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(UnorderedMap::default()),
        }
    }

    pub fn with_restored(
        generator_name: impl Into<String>,
        targets: UnorderedMap<String, OmniPath>,
        inputs: UnorderedMap<String, serde_json::Value>,
    ) -> Self {
        let data = UnorderedMap::from_iter([(
            generator_name.into(),
            DataImpl { inputs, targets },
        )]);
        Self {
            data: Mutex::new(data),
        }
    }

    pub async fn from_disk<'a, TPath, TSys>(
        path: TPath,
        sys: &TSys,
    ) -> Result<Self, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + Send + Sync,
        TPath: Into<&'a Path>,
    {
        let result: UnorderedMap<String, DataImpl> =
            omni_file_data_serde::read_async(path, sys).await?;

        Ok(Self {
            data: Mutex::new(result),
        })
    }
}

impl GenSession {
    pub fn set_target(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<OmniPath>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .targets
            .insert(key.into(), value.into());
    }

    pub fn get_target(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<OmniPath> {
        self.data
            .lock()
            .unwrap()
            .get(generator.as_ref())
            .and_then(|d| d.targets.get(key.as_ref()))
            .map(|p| p.clone())
    }

    pub fn set_input_raw(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs
            .insert(key.into(), value.into());
    }

    pub fn set_input(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Serialize,
    ) -> Result<(), serde_json::Error> {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs
            .insert(key.into(), serde_json::to_value(value)?);
        Ok(())
    }

    pub fn get_input_raw(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<serde_json::Value> {
        self.data
            .lock()
            .unwrap()
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .map(|p| p.clone())
    }

    pub fn get_input<T: serde::de::DeserializeOwned>(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<Result<T, serde_json::Error>> {
        self.data
            .lock()
            .unwrap()
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .map(|p| serde_json::from_value(p.clone()))
    }

    pub fn set_inputs_raw(
        &self,
        generator: impl Into<String>,
        inputs: UnorderedMap<String, serde_json::Value>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs = inputs;
    }

    pub fn set_inputs(
        &self,
        generator: impl Into<String>,
        inputs: UnorderedMap<String, impl Serialize>,
    ) -> Result<(), serde_json::Error> {
        let mut transformed = UnorderedMap::default();
        for (key, value) in inputs {
            transformed.insert(key, serde_json::to_value(value)?);
        }

        self.set_inputs_raw(generator, transformed);

        Ok(())
    }

    pub fn set_targets(
        &self,
        generator: impl Into<String>,
        targets: UnorderedMap<String, OmniPath>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .targets = targets;
    }

    pub fn merge(&self, other: GenSession) {
        let mut data = self.data.lock().unwrap();
        let other = other.data.lock().unwrap();

        for (generator, other_data) in other.iter() {
            let data = data
                .entry(generator.clone())
                .or_insert_with(DataImpl::default);
            data.targets.extend(other_data.targets.clone());
            data.inputs.extend(other_data.inputs.clone());
        }
    }

    pub fn restore_targets(
        &self,
        generator: impl AsRef<str>,
        targets: &mut UnorderedMap<String, OmniPath>,
        override_existing: bool,
    ) {
        let data = self.data.lock().unwrap();
        let data = data.get(generator.as_ref()).map(|d| &d.targets);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !targets.contains_key(k) {
                    targets.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub fn restore_inputs(
        &self,
        generator: impl AsRef<str>,
        inputs: &mut UnorderedMap<String, serde_json::Value>,
        override_existing: bool,
    ) {
        let data = self.data.lock().unwrap();
        let data = data.get(generator.as_ref()).map(|d| &d.inputs);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !inputs.contains_key(k) {
                    inputs.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub fn restore_inputs_as_value_bag(
        &self,
        generator: impl AsRef<str>,
        inputs: &mut UnorderedMap<String, OwnedValueBag>,
        override_existing: bool,
    ) {
        let data = self.data.lock().unwrap();
        let data = data.get(generator.as_ref()).map(|d| &d.inputs);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !inputs.contains_key(k) {
                    inputs.insert(
                        k.clone(),
                        ValueBag::capture_serde1(v).to_owned(),
                    );
                }
            }
        }
    }

    pub async fn write_to_disk<'a, TPath, TSys>(
        &self,
        path: TPath,
        sys: &TSys,
    ) -> Result<(), omni_file_data_serde::Error>
    where
        TSys: FsWriteAsync + Send + Sync,
        TPath: Into<&'a Path>,
    {
        let data = self.data.lock().unwrap();
        omni_file_data_serde::write_async(path, &*data, sys).await?;

        Ok(())
    }

    pub fn unset_targets(
        &self,
        generator: impl Into<String>,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        let mut data = self.data.lock().unwrap();
        if let Some(data) = data.get_mut(generator.into().as_str()) {
            for key in keys {
                data.targets.remove(key.as_ref());
            }
        }
    }

    pub fn unset_inputs(
        &self,
        generator: impl Into<String>,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        let mut data = self.data.lock().unwrap();
        if let Some(data) = data.get_mut(generator.into().as_str()) {
            for key in keys {
                data.inputs.remove(key.as_ref());
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        let data = self.data.lock().unwrap();
        if data.is_empty() {
            return true;
        }

        for (_, data) in data.iter() {
            if !data.targets.is_empty() || !data.inputs.is_empty() {
                return false;
            }
        }

        return true;
    }

    pub async fn has_changes<'a, TPath, TSys>(
        &self,
        serialized_file_path: TPath,
        sys: &TSys,
    ) -> Result<bool, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + Send + Sync,
        TPath: Into<&'a Path>,
    {
        let data = self.data.lock().unwrap();
        let original: UnorderedMap<String, DataImpl> =
            omni_file_data_serde::read_async(serialized_file_path, sys).await?;

        if data.len() != original.len() {
            return Ok(true);
        }

        for (generator, data) in data.iter() {
            let original = original.get(generator);
            if original.is_none() {
                return Ok(true);
            }
            let original = original.unwrap();

            if data.targets != original.targets
                || data.inputs != original.inputs
            {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

#[derive(
    Clone, serde::Serialize, serde::Deserialize, Default, Debug, PartialEq,
)]
struct DataImpl {
    targets: UnorderedMap<String, OmniPath>,
    #[serde(alias = "prompts")]
    inputs: UnorderedMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use omni_types::OmniPath;
    use serde::{Deserialize, Serialize};
    use system_traits::{FsCreateDirAll as _, impls::InMemorySys};
    use value_bag::{OwnedValueBag, ValueBag};

    use super::*;

    fn make_sys() -> (InMemorySys, &'static Path) {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all(Path::new("/sessions"))
            .expect("create dir");
        (sys, Path::new("/sessions/session.json"))
    }

    // ── new() ────────────────────────────────────────────────────────────────

    #[test]
    fn test_new_is_empty() {
        assert!(GenSession::new().is_empty());
    }

    #[test]
    fn test_with_restored_stores_targets() {
        let mut targets = UnorderedMap::default();
        targets.insert("output".to_string(), OmniPath::new("dist/file.txt"));

        let session = GenSession::with_restored(
            "gen_a",
            targets,
            UnorderedMap::default(),
        );

        assert_eq!(
            session.get_target("gen_a", "output"),
            Some(OmniPath::new("dist/file.txt"))
        );
    }

    #[test]
    fn test_with_restored_stores_inputs() {
        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Alice"));

        let session =
            GenSession::with_restored("gen_a", UnorderedMap::default(), inputs);

        assert_eq!(
            session.get_input_raw("gen_a", "name"),
            Some(serde_json::json!("Alice"))
        );
    }

    // ── with_restored() ──────────────────────────────────────────────────────

    #[test]
    fn test_with_restored_is_not_empty() {
        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("a.txt"));

        let session = GenSession::with_restored(
            "gen_a",
            targets,
            UnorderedMap::default(),
        );
        assert!(!session.is_empty());
    }

    // ── set_target / get_target ──────────────────────────────────────────────

    #[test]
    fn test_set_get_target_basic() {
        let session = GenSession::new();
        session.set_target("gen_a", "output", OmniPath::new("dist/file.txt"));

        assert_eq!(
            session.get_target("gen_a", "output"),
            Some(OmniPath::new("dist/file.txt"))
        );
    }

    #[test]
    fn test_get_target_missing_generator_returns_none() {
        let session = GenSession::new();
        assert_eq!(session.get_target("no_such_gen", "output"), None);
    }

    #[test]
    fn test_get_target_missing_key_returns_none() {
        let session = GenSession::new();
        session.set_target("gen_a", "output", OmniPath::new("dist/file.txt"));
        assert_eq!(session.get_target("gen_a", "no_such_key"), None);
    }

    #[test]
    fn test_set_target_overwrites_existing() {
        let session = GenSession::new();
        session.set_target("gen_a", "output", OmniPath::new("v1.txt"));
        session.set_target("gen_a", "output", OmniPath::new("v2.txt"));

        assert_eq!(
            session.get_target("gen_a", "output"),
            Some(OmniPath::new("v2.txt"))
        );
    }

    #[test]
    fn test_set_target_multiple_generators_are_isolated() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.set_target("gen_b", "out", OmniPath::new("b.txt"));

        assert_eq!(
            session.get_target("gen_a", "out"),
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            session.get_target("gen_b", "out"),
            Some(OmniPath::new("b.txt"))
        );
    }

    // ── set_input_raw / get_input_raw ─────────────────────────────────────────

    #[test]
    fn test_set_get_input_raw_string() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));

        assert_eq!(
            session.get_input_raw("gen_a", "name"),
            Some(serde_json::json!("Alice"))
        );
    }

    #[test]
    fn test_set_get_input_raw_number() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "count", serde_json::json!(42));

        assert_eq!(
            session.get_input_raw("gen_a", "count"),
            Some(serde_json::json!(42))
        );
    }

    #[test]
    fn test_set_get_input_raw_object() {
        let session = GenSession::new();
        let val = serde_json::json!({ "x": 1, "y": [true, null] });
        session.set_input_raw("gen_a", "cfg", val.clone());

        assert_eq!(session.get_input_raw("gen_a", "cfg"), Some(val));
    }

    #[test]
    fn test_get_input_raw_missing_generator_returns_none() {
        let session = GenSession::new();
        assert_eq!(session.get_input_raw("no_gen", "key"), None);
    }

    #[test]
    fn test_get_input_raw_missing_key_returns_none() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));
        assert_eq!(session.get_input_raw("gen_a", "no_key"), None);
    }

    #[test]
    fn test_set_input_raw_overwrites_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "k", serde_json::json!(1));
        session.set_input_raw("gen_a", "k", serde_json::json!(2));

        assert_eq!(
            session.get_input_raw("gen_a", "k"),
            Some(serde_json::json!(2))
        );
    }

    // ── set_input / get_input (typed) ─────────────────────────────────────────

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestConfig {
        name: String,
        value: u32,
    }

    #[test]
    fn test_set_get_input_typed_round_trip() {
        let session = GenSession::new();
        let cfg = TestConfig {
            name: "hello".to_string(),
            value: 99,
        };

        session.set_input("gen_a", "config", &cfg).unwrap();
        let got: TestConfig = session
            .get_input::<TestConfig>("gen_a", "config")
            .unwrap()
            .unwrap();

        assert_eq!(got, cfg);
    }

    #[test]
    fn test_get_input_missing_generator_returns_none() {
        let session = GenSession::new();
        assert!(session.get_input::<String>("no_gen", "key").is_none());
    }

    #[test]
    fn test_get_input_missing_key_returns_none() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "other", serde_json::json!("x"));
        assert!(
            session
                .get_input::<String>("gen_a", "no_such_key")
                .is_none()
        );
    }

    #[test]
    fn test_get_input_type_mismatch_returns_err() {
        let session = GenSession::new();
        // Store a plain number; try to deserialize as a struct.
        session.set_input_raw("gen_a", "num", serde_json::json!(42));

        let result = session.get_input::<TestConfig>("gen_a", "num");
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    // ── set_inputs / set_targets (bulk replace) ───────────────────────────────

    #[test]
    fn test_set_inputs_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "old_key", serde_json::json!("old"));

        let mut new_inputs = UnorderedMap::default();
        new_inputs.insert("new_key".to_string(), serde_json::json!("new_val"));
        session
            .set_inputs("gen_a", new_inputs)
            .expect("should succeed");

        assert_eq!(session.get_input_raw("gen_a", "old_key"), None);
        assert_eq!(
            session.get_input_raw("gen_a", "new_key"),
            Some(serde_json::json!("new_val"))
        );
    }

    #[test]
    fn test_set_targets_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session.set_target("gen_a", "old_out", OmniPath::new("old.txt"));

        let mut new_targets = UnorderedMap::default();
        new_targets.insert("new_out".to_string(), OmniPath::new("new.txt"));
        session.set_targets("gen_a", new_targets);

        assert_eq!(session.get_target("gen_a", "old_out"), None);
        assert_eq!(
            session.get_target("gen_a", "new_out"),
            Some(OmniPath::new("new.txt"))
        );
    }

    // ── merge() ───────────────────────────────────────────────────────────────

    #[test]
    fn test_merge_combines_disjoint_generators() {
        let session = GenSession::new();
        session.set_target("a", "a", OmniPath::new("a"));
        session.set_target("b", "b", OmniPath::new("b"));

        let other = GenSession::new();
        other.set_target("a", "b", OmniPath::new("b"));
        other.set_target("c", "c", OmniPath::new("c"));

        session.merge(other);

        assert_eq!(session.get_target("a", "a"), Some(OmniPath::new("a")));
        assert_eq!(session.get_target("a", "b"), Some(OmniPath::new("b")));
        assert_eq!(session.get_target("b", "b"), Some(OmniPath::new("b")));
        assert_eq!(session.get_target("c", "c"), Some(OmniPath::new("c")));
    }

    #[test]
    fn test_merge_same_key_other_wins() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("original.txt"));

        let other = GenSession::new();
        other.set_target("gen_a", "out", OmniPath::new("overridden.txt"));

        session.merge(other);

        assert_eq!(
            session.get_target("gen_a", "out"),
            Some(OmniPath::new("overridden.txt"))
        );
    }

    #[test]
    fn test_merge_inputs_are_combined() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "x", serde_json::json!(1));

        let other = GenSession::new();
        other.set_input_raw("gen_a", "y", serde_json::json!(2));

        session.merge(other);

        assert_eq!(
            session.get_input_raw("gen_a", "x"),
            Some(serde_json::json!(1))
        );
        assert_eq!(
            session.get_input_raw("gen_a", "y"),
            Some(serde_json::json!(2))
        );
    }

    #[test]
    fn test_merge_empty_other_is_no_op() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));

        session.merge(GenSession::new());

        assert_eq!(
            session.get_target("gen_a", "out"),
            Some(OmniPath::new("a.txt"))
        );
    }

    #[test]
    fn test_merge_into_empty_session() {
        let session = GenSession::new();

        let other = GenSession::new();
        other.set_target("gen_a", "out", OmniPath::new("a.txt"));
        other.set_input_raw("gen_a", "k", serde_json::json!("v"));

        session.merge(other);

        assert_eq!(
            session.get_target("gen_a", "out"),
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            session.get_input_raw("gen_a", "k"),
            Some(serde_json::json!("v"))
        );
    }

    // ── restore_targets() ─────────────────────────────────────────────────────

    #[test]
    fn test_restore_targets_fills_missing_keys() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("session.txt"));

        let mut targets = UnorderedMap::default();
        session.restore_targets("gen_a", &mut targets, false);

        assert_eq!(targets.get("out"), Some(&OmniPath::new("session.txt")));
    }

    #[test]
    fn test_restore_targets_no_override_preserves_existing() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("session.txt"));

        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));
        session.restore_targets("gen_a", &mut targets, false);

        assert_eq!(targets.get("out"), Some(&OmniPath::new("existing.txt")));
    }

    #[test]
    fn test_restore_targets_with_override_replaces_existing() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("session.txt"));

        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));
        session.restore_targets("gen_a", &mut targets, true);

        assert_eq!(targets.get("out"), Some(&OmniPath::new("session.txt")));
    }

    #[test]
    fn test_restore_targets_missing_generator_is_no_op() {
        let session = GenSession::new();
        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));

        session.restore_targets("no_such_gen", &mut targets, true);

        assert_eq!(targets.get("out"), Some(&OmniPath::new("existing.txt")));
    }

    #[test]
    fn test_restore_targets_no_override_fills_missing_leaves_others() {
        let session = GenSession::new();
        session.set_target("gen_a", "from_session", OmniPath::new("s.txt"));
        session.set_target("gen_a", "both", OmniPath::new("session_both.txt"));

        let mut targets = UnorderedMap::default();
        targets.insert("both".to_string(), OmniPath::new("existing_both.txt"));

        session.restore_targets("gen_a", &mut targets, false);

        // Key only in session is filled in.
        assert_eq!(targets.get("from_session"), Some(&OmniPath::new("s.txt")));
        // Key present in both: existing wins (no override).
        assert_eq!(
            targets.get("both"),
            Some(&OmniPath::new("existing_both.txt"))
        );
    }

    // ── restore_inputs() ──────────────────────────────────────────────────────

    #[test]
    fn test_restore_inputs_fills_missing_keys() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));

        let mut inputs = UnorderedMap::default();
        session.restore_inputs("gen_a", &mut inputs, false);

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Alice")));
    }

    #[test]
    fn test_restore_inputs_no_override_preserves_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));

        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));
        session.restore_inputs("gen_a", &mut inputs, false);

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Bob")));
    }

    #[test]
    fn test_restore_inputs_with_override_replaces_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));

        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));
        session.restore_inputs("gen_a", &mut inputs, true);

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Alice")));
    }

    #[test]
    fn test_restore_inputs_missing_generator_is_no_op() {
        let session = GenSession::new();
        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));

        session.restore_inputs("no_such_gen", &mut inputs, true);

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Bob")));
    }

    // ── restore_inputs_as_value_bag() ─────────────────────────────────────────

    #[test]
    fn test_restore_inputs_as_value_bag_fills_missing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "count", serde_json::json!(7));

        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        session.restore_inputs_as_value_bag("gen_a", &mut map, false);

        assert!(map.contains_key("count"));
        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(7));
    }

    #[test]
    fn test_restore_inputs_as_value_bag_no_override_preserves_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "count", serde_json::json!(99));

        let existing =
            ValueBag::capture_serde1(&serde_json::json!(1)).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("count".to_string(), existing);

        session.restore_inputs_as_value_bag("gen_a", &mut map, false);

        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(1));
    }

    #[test]
    fn test_restore_inputs_as_value_bag_with_override() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "count", serde_json::json!(99));

        let existing =
            ValueBag::capture_serde1(&serde_json::json!(1)).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("count".to_string(), existing);

        session.restore_inputs_as_value_bag("gen_a", &mut map, true);

        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(99));
    }

    // ── unset_targets() ───────────────────────────────────────────────────────

    #[test]
    fn test_unset_targets_removes_specified_key() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.set_target("gen_a", "other", OmniPath::new("b.txt"));

        session.unset_targets("gen_a", ["out".to_string()]);

        assert_eq!(session.get_target("gen_a", "out"), None);
        assert_eq!(
            session.get_target("gen_a", "other"),
            Some(OmniPath::new("b.txt"))
        );
    }

    #[test]
    fn test_unset_targets_missing_generator_does_not_panic() {
        let session = GenSession::new();
        session.unset_targets("no_such_gen", ["key".to_string()]);
    }

    #[test]
    fn test_unset_targets_multiple_keys() {
        let session = GenSession::new();
        session.set_target("gen_a", "a", OmniPath::new("a.txt"));
        session.set_target("gen_a", "b", OmniPath::new("b.txt"));
        session.set_target("gen_a", "c", OmniPath::new("c.txt"));

        session.unset_targets("gen_a", ["a".to_string(), "b".to_string()]);

        assert_eq!(session.get_target("gen_a", "a"), None);
        assert_eq!(session.get_target("gen_a", "b"), None);
        assert_eq!(
            session.get_target("gen_a", "c"),
            Some(OmniPath::new("c.txt"))
        );
    }

    // ── unset_inputs() ────────────────────────────────────────────────────────

    #[test]
    fn test_unset_inputs_removes_specified_key() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "x", serde_json::json!(1));
        session.set_input_raw("gen_a", "y", serde_json::json!(2));

        session.unset_inputs("gen_a", ["x".to_string()]);

        assert_eq!(session.get_input_raw("gen_a", "x"), None);
        assert_eq!(
            session.get_input_raw("gen_a", "y"),
            Some(serde_json::json!(2))
        );
    }

    #[test]
    fn test_unset_inputs_missing_generator_does_not_panic() {
        let session = GenSession::new();
        session.unset_inputs("no_such_gen", ["key".to_string()]);
    }

    // ── is_empty() ────────────────────────────────────────────────────────────

    #[test]
    fn test_is_empty_new_session() {
        assert!(GenSession::new().is_empty());
    }

    #[test]
    fn test_is_empty_false_after_adding_target() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        assert!(!session.is_empty());
    }

    #[test]
    fn test_is_empty_false_after_adding_input() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "k", serde_json::json!("v"));
        assert!(!session.is_empty());
    }

    #[test]
    fn test_is_empty_true_after_removing_only_target() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.unset_targets("gen_a", ["out".to_string()]);

        // Generator entry exists but has no targets or inputs.
        assert!(session.is_empty());
    }

    #[test]
    fn test_is_empty_false_when_only_input_present() {
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.unset_targets("gen_a", ["out".to_string()]);
        session.set_input_raw("gen_a", "k", serde_json::json!("v"));

        assert!(!session.is_empty());
    }

    // ── has_changes() ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_has_changes_false_when_data_matches_disk() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.set_input_raw("gen_a", "k", serde_json::json!("v"));
        session.write_to_disk(path, &sys).await.unwrap();

        assert!(!session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_false_for_empty_session_and_empty_disk() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.write_to_disk(path, &sys).await.unwrap();

        assert!(!session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_adding_target() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.set_target("gen_a", "extra", OmniPath::new("extra.txt"));
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_adding_input() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.set_input_raw("gen_a", "k", serde_json::json!("new"));
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_modifying_target_value() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("original.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.set_target("gen_a", "out", OmniPath::new("modified.txt"));
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_modifying_input_value() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_input_raw("gen_a", "k", serde_json::json!("original"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.set_input_raw("gen_a", "k", serde_json::json!("changed"));
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_removing_target() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.unset_targets("gen_a", ["out"]);
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_removing_input() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_input_raw("gen_a", "k", serde_json::json!("v"));
        session.write_to_disk(path, &sys).await.unwrap();

        session.unset_inputs("gen_a", ["k"]);
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_adding_new_generator() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        // gen_b is new – data.len() now exceeds what is on disk.
        session.set_target("gen_b", "out", OmniPath::new("b.txt"));
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_returns_err_for_missing_file() {
        let sys = InMemorySys::default();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));

        // File was never written – reading should fail.
        let result = session
            .has_changes(Path::new("/nonexistent.json"), &sys)
            .await;
        assert!(result.is_err());
    }

    // ── write_to_disk / from_disk ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_disk_round_trip_json() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("dist/file.txt"));
        session.set_input_raw("gen_a", "name", serde_json::json!("Alice"));

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "out"),
            Some(OmniPath::new("dist/file.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_a", "name"),
            Some(serde_json::json!("Alice"))
        );
    }

    #[tokio::test]
    async fn test_disk_round_trip_multiple_generators() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "a_out", OmniPath::new("a.txt"));
        session.set_target("gen_b", "b_out", OmniPath::new("b.txt"));
        session.set_input_raw("gen_b", "mode", serde_json::json!("fast"));

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "a_out"),
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            loaded.get_target("gen_b", "b_out"),
            Some(OmniPath::new("b.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_b", "mode"),
            Some(serde_json::json!("fast"))
        );
    }

    #[tokio::test]
    async fn test_has_changes_false_for_just_loaded_session() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("dist/file.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        let loaded = GenSession::from_disk(path, &sys).await.unwrap();
        assert!(!loaded.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_for_loaded_then_mutated_session() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("dist/file.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        let loaded = GenSession::from_disk(path, &sys).await.unwrap();
        loaded.set_target("gen_a", "new_key", OmniPath::new("other.txt"));
        assert!(loaded.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_disk_round_trip_yaml() {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all(Path::new("/sessions"))
            .expect("create dir");
        let path = Path::new("/sessions/session.yaml");

        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("build/result.txt"));
        session.set_input_raw("gen_a", "env", serde_json::json!("production"));

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "out"),
            Some(OmniPath::new("build/result.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_a", "env"),
            Some(serde_json::json!("production"))
        );
    }

    #[tokio::test]
    async fn test_disk_round_trip_preserves_complex_inputs() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        let complex = serde_json::json!({ "list": [1, "two", null], "nested": { "ok": true } });
        session.set_input_raw("gen_a", "cfg", complex.clone());

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(loaded.get_input_raw("gen_a", "cfg"), Some(complex));
    }

    // ── set_inputs_raw() ──────────────────────────────────────────────────────

    #[test]
    fn test_set_inputs_raw_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "old_key", serde_json::json!("old"));

        let mut new_inputs = UnorderedMap::default();
        new_inputs.insert("new_key".to_string(), serde_json::json!("new_val"));
        session.set_inputs_raw("gen_a", new_inputs);

        assert_eq!(session.get_input_raw("gen_a", "old_key"), None);
        assert_eq!(
            session.get_input_raw("gen_a", "new_key"),
            Some(serde_json::json!("new_val"))
        );
    }

    #[test]
    fn test_set_inputs_raw_creates_generator_if_absent() {
        let session = GenSession::new();
        let mut inputs = UnorderedMap::default();
        inputs.insert("k".to_string(), serde_json::json!(42));
        session.set_inputs_raw("gen_a", inputs);

        assert_eq!(
            session.get_input_raw("gen_a", "k"),
            Some(serde_json::json!(42))
        );
    }

    #[test]
    fn test_set_inputs_raw_with_empty_map_clears_inputs() {
        let session = GenSession::new();
        session.set_input_raw("gen_a", "k", serde_json::json!(1));
        session.set_inputs_raw("gen_a", UnorderedMap::default());

        assert_eq!(session.get_input_raw("gen_a", "k"), None);
    }

    // ── set_inputs() typed overload ───────────────────────────────────────────

    #[test]
    fn test_set_inputs_typed_serializes_struct_values() {
        let session = GenSession::new();
        let cfg = TestConfig {
            name: "world".to_string(),
            value: 7,
        };

        let mut inputs = UnorderedMap::default();
        inputs.insert("cfg".to_string(), cfg);
        session.set_inputs("gen_a", inputs).unwrap();

        let got: TestConfig = session
            .get_input::<TestConfig>("gen_a", "cfg")
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            TestConfig {
                name: "world".to_string(),
                value: 7
            }
        );
    }

    #[test]
    fn test_set_inputs_typed_returns_ok_for_serializable_values() {
        let session = GenSession::new();
        let mut inputs: UnorderedMap<String, serde_json::Value> =
            UnorderedMap::default();
        inputs.insert("k".to_string(), serde_json::json!(99));
        assert!(session.set_inputs("gen_a", inputs).is_ok());
    }

    // ── restore_inputs_as_value_bag() – missing generator ─────────────────────

    #[test]
    fn test_restore_inputs_as_value_bag_missing_generator_is_no_op() {
        let session = GenSession::new();
        let existing =
            ValueBag::capture_serde1(&serde_json::json!("kept")).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("k".to_string(), existing);

        session.restore_inputs_as_value_bag("no_such_gen", &mut map, true);

        // Map is unchanged.
        assert!(map.contains_key("k"));
        assert_eq!(map.len(), 1);
    }

    // ── has_changes() after merge ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_has_changes_true_after_merge_adds_data() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.write_to_disk(path, &sys).await.unwrap();

        let other = GenSession::new();
        other.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.merge(other);

        // Disk is empty; session now has gen_a.
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_merge_adds_new_generator() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.write_to_disk(path, &sys).await.unwrap();

        let other = GenSession::new();
        other.set_target("gen_b", "out", OmniPath::new("b.txt"));
        session.merge(other);

        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    // ── is_empty() – multi-generator ──────────────────────────────────────────

    #[test]
    fn test_is_empty_false_when_only_second_generator_has_data() {
        let session = GenSession::new();
        // gen_a gets a target that is then removed (empty entry remains).
        session.set_target("gen_a", "out", OmniPath::new("a.txt"));
        session.unset_targets("gen_a", ["out"]);
        // gen_b has live data.
        session.set_target("gen_b", "out", OmniPath::new("b.txt"));

        assert!(!session.is_empty());
    }

    #[test]
    fn test_is_empty_true_when_all_generators_are_empty() {
        let session = GenSession::new();
        session.set_target("gen_a", "a", OmniPath::new("a.txt"));
        session.set_target("gen_b", "b", OmniPath::new("b.txt"));
        session.unset_targets("gen_a", ["a"]);
        session.unset_targets("gen_b", ["b"]);

        assert!(session.is_empty());
    }
}
