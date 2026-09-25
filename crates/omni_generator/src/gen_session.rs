use std::path::Path;

use maps::UnorderedMap;
use omni_generator_configurations::OmniPath;
use serde::Serialize;
use sets::UnorderedSet;
use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsReadAsync, FsRemoveFileAsync,
    FsWriteAsync,
};
use tokio::sync::Mutex;
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

    pub async fn from_disk<TPath, TSys>(
        path: TPath,
        sys: &TSys,
    ) -> Result<Self, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        let result: SessionFile =
            omni_file_data_serde::read_async(path, sys).await?;

        Ok(Self {
            data: Mutex::new(result.generators),
        })
    }
}

impl GenSession {
    pub async fn set_target(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<OmniPath>,
    ) {
        self.data
            .lock()
            .await
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .targets
            .insert(key.into(), value.into());
    }

    pub async fn get_target(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<OmniPath> {
        self.data
            .lock()
            .await
            .get(generator.as_ref())
            .and_then(|d| d.targets.get(key.as_ref()))
            .cloned()
    }

    pub async fn set_input_raw(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) {
        self.data
            .lock()
            .await
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs
            .insert(key.into(), value.into());
    }

    pub async fn set_input(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Serialize,
    ) -> Result<(), serde_json::Error> {
        self.data
            .lock()
            .await
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs
            .insert(key.into(), serde_json::to_value(value)?);
        Ok(())
    }

    pub async fn get_input_raw(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<serde_json::Value> {
        self.data
            .lock()
            .await
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .cloned()
    }

    pub async fn get_input<T: serde::de::DeserializeOwned>(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<Result<T, serde_json::Error>> {
        self.data
            .lock()
            .await
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .map(|p| serde_json::from_value(p.clone()))
    }

    pub async fn set_inputs_raw(
        &self,
        generator: impl Into<String>,
        inputs: UnorderedMap<String, serde_json::Value>,
    ) {
        self.data
            .lock()
            .await
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .inputs = inputs;
    }

    pub async fn set_inputs(
        &self,
        generator: impl Into<String>,
        inputs: UnorderedMap<String, impl Serialize>,
    ) -> Result<(), serde_json::Error> {
        let mut transformed = UnorderedMap::default();
        for (key, value) in inputs {
            transformed.insert(key, serde_json::to_value(value)?);
        }

        self.set_inputs_raw(generator, transformed).await;

        Ok(())
    }

    pub async fn set_targets(
        &self,
        generator: impl Into<String>,
        targets: UnorderedMap<String, OmniPath>,
    ) {
        self.data
            .lock()
            .await
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .targets = targets;
    }

    pub async fn merge(&self, other: GenSession) {
        let mut data = self.data.lock().await;
        let other = other.data.lock().await;

        for (generator, other_data) in other.iter() {
            let data = data
                .entry(generator.clone())
                .or_insert_with(DataImpl::default);
            data.targets.extend(other_data.targets.clone());
            data.inputs.extend(other_data.inputs.clone());
        }
    }

    pub async fn restore_targets(
        &self,
        generator: impl AsRef<str>,
        targets: &mut UnorderedMap<String, OmniPath>,
        override_existing: bool,
    ) {
        let data = self.data.lock().await;
        let data = data.get(generator.as_ref()).map(|d| &d.targets);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !targets.contains_key(k) {
                    targets.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub async fn restore_inputs(
        &self,
        generator: impl AsRef<str>,
        inputs: &mut UnorderedMap<String, serde_json::Value>,
        override_existing: bool,
    ) {
        let data = self.data.lock().await;
        let data = data.get(generator.as_ref()).map(|d| &d.inputs);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !inputs.contains_key(k) {
                    inputs.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub async fn restore_inputs_as_value_bag(
        &self,
        generator: impl AsRef<str>,
        inputs: &mut UnorderedMap<String, OwnedValueBag>,
        override_existing: bool,
    ) {
        let data = self.data.lock().await;
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

    pub async fn write_to_disk<TPath, TSys>(
        &self,
        path: TPath,
        sys: &TSys,
    ) -> Result<(), omni_file_data_serde::Error>
    where
        TSys: FsWriteAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        let data = self.data.lock().await.clone();
        omni_file_data_serde::write_async(path, &data, sys).await?;

        Ok(())
    }

    pub async fn unset_targets(
        &self,
        generator: impl Into<String>,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        let mut data = self.data.lock().await;
        if let Some(data) = data.get_mut(generator.into().as_str()) {
            for key in keys {
                data.targets.remove(key.as_ref());
            }
        }
    }

    pub async fn unset_inputs(
        &self,
        generator: impl Into<String>,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        let mut data = self.data.lock().await;
        if let Some(data) = data.get_mut(generator.into().as_str()) {
            for key in keys {
                data.inputs.remove(key.as_ref());
            }
        }
    }

    pub async fn is_empty(&self) -> bool {
        let data = self.data.lock().await;
        if data.is_empty() {
            return true;
        }

        for data in data.values() {
            if !data.targets.is_empty() || !data.inputs.is_empty() {
                return false;
            }
        }

        true
    }

    pub async fn has_changes<TPath, TSys>(
        &self,
        serialized_file_path: TPath,
        sys: &TSys,
    ) -> Result<bool, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        let data = self.data.lock().await.clone();
        let original: SessionFile =
            omni_file_data_serde::read_async(serialized_file_path, sys).await?;
        let original = original.generators;

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

    pub async fn is_root_marked<TPath, TSys>(
        path: TPath,
        sys: &TSys,
    ) -> Result<bool, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + FsMetadataAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        Ok(SessionFile::load_or_default(path, sys).await?.root)
    }

    pub async fn retain_delta_against(
        &self,
        baseline: &GenSession,
        pinned_inputs: &UnorderedSet<String>,
        pinned_targets: &UnorderedSet<String>,
    ) {
        let mut data = self.data.lock().await;
        let baseline = baseline.data.lock().await;

        let generator_names: Vec<String> = data.keys().cloned().collect();

        for generator in generator_names {
            let Some(base) = baseline.get(&generator) else {
                continue;
            };

            let entry = data
                .get_mut(&generator)
                .expect("generator name taken from this map");

            let target_keys: Vec<String> =
                entry.targets.keys().cloned().collect();
            for key in target_keys {
                if pinned_targets.contains(&key) {
                    continue;
                }
                if base.targets.get(&key) == entry.targets.get(&key) {
                    entry.targets.remove(&key);
                }
            }

            let input_keys: Vec<String> =
                entry.inputs.keys().cloned().collect();
            for key in input_keys {
                if pinned_inputs.contains(&key) {
                    continue;
                }
                if base.inputs.get(&key) == entry.inputs.get(&key) {
                    entry.inputs.remove(&key);
                }
            }

            if entry.targets.is_empty() && entry.inputs.is_empty() {
                data.remove(&generator);
            }
        }
    }

    pub async fn delta_differs_from_disk<TPath, TSys>(
        &self,
        path: TPath,
        sys: &TSys,
    ) -> Result<bool, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + FsMetadataAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        let existing = SessionFile::load_or_default(&path, sys).await?;
        let data = self.data.lock().await;
        Ok(*data != existing.generators)
    }

    pub async fn write_delta_or_prune<TSys>(
        &self,
        session_file_path: impl AsRef<Path>,
        gen_dir: impl AsRef<Path>,
        sys: &TSys,
    ) -> Result<DeltaSaveOutcome, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync
            + FsWriteAsync
            + FsMetadataAsync
            + FsRemoveFileAsync
            + FsCreateDirAllAsync
            + Send
            + Sync,
    {
        let path = session_file_path.as_ref();
        let existing = SessionFile::load_or_default(path, sys).await?;
        let data = self.data.lock().await;

        if *data == existing.generators {
            return Ok(DeltaSaveOutcome::Unchanged);
        }

        let is_empty = data
            .values()
            .all(|d| d.targets.is_empty() && d.inputs.is_empty());

        if is_empty && !existing.root {
            sys.fs_remove_file_async(path).await?;
            return Ok(DeltaSaveOutcome::Pruned);
        }

        let out = SessionFile {
            root: existing.root,
            generators: data.clone(),
        };
        drop(data);

        sys.fs_create_dir_all_async(gen_dir.as_ref()).await?;
        omni_file_data_serde::write_async(path, &out, sys).await?;

        Ok(DeltaSaveOutcome::Wrote)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaSaveOutcome {
    Unchanged,
    Wrote,
    Pruned,
}

#[derive(
    Clone, serde::Serialize, serde::Deserialize, Default, Debug, PartialEq,
)]
struct DataImpl {
    #[serde(default)]
    targets: UnorderedMap<String, OmniPath>,
    #[serde(default, alias = "prompts")]
    inputs: UnorderedMap<String, serde_json::Value>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq,
)]
struct SessionFile {
    #[serde(default, skip_serializing_if = "is_false")]
    root: bool,
    #[serde(flatten)]
    generators: UnorderedMap<String, DataImpl>,
}

impl SessionFile {
    async fn load_or_default<TPath, TSys>(
        path: TPath,
        sys: &TSys,
    ) -> Result<Self, omni_file_data_serde::Error>
    where
        TSys: FsReadAsync + FsMetadataAsync + Send + Sync,
        TPath: AsRef<Path>,
    {
        if !sys.fs_exists_no_err_async(path.as_ref()).await {
            return Ok(Self::default());
        }
        omni_file_data_serde::read_async(path, sys).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use omni_types::OmniPath;
    use serde::{Deserialize, Serialize};
    use system_traits::{
        FsCreateDirAll as _, FsMetadataAsync as _, FsReadAsync as _,
        FsWriteAsync as _, impls::InMemorySys,
    };
    use value_bag::{OwnedValueBag, ValueBag};

    use super::*;

    fn make_sys() -> (InMemorySys, &'static Path) {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all(Path::new("/sessions"))
            .expect("create dir");
        (sys, Path::new("/sessions/session.json"))
    }

    // ── new() ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_new_is_empty() {
        assert!(GenSession::new().is_empty().await);
    }

    #[tokio::test]
    async fn test_with_restored_stores_targets() {
        let mut targets = UnorderedMap::default();
        targets.insert("output".to_string(), OmniPath::new("dist/file.txt"));

        let session = GenSession::with_restored(
            "gen_a",
            targets,
            UnorderedMap::default(),
        );

        assert_eq!(
            session.get_target("gen_a", "output").await,
            Some(OmniPath::new("dist/file.txt"))
        );
    }

    #[tokio::test]
    async fn test_with_restored_stores_inputs() {
        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Alice"));

        let session =
            GenSession::with_restored("gen_a", UnorderedMap::default(), inputs);

        assert_eq!(
            session.get_input_raw("gen_a", "name").await,
            Some(serde_json::json!("Alice"))
        );
    }

    // ── with_restored() ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_with_restored_is_not_empty() {
        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("a.txt"));

        let session = GenSession::with_restored(
            "gen_a",
            targets,
            UnorderedMap::default(),
        );
        assert!(!session.is_empty().await);
    }

    // ── set_target / get_target ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_get_target_basic() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "output", OmniPath::new("dist/file.txt"))
            .await;

        assert_eq!(
            session.get_target("gen_a", "output").await,
            Some(OmniPath::new("dist/file.txt"))
        );
    }

    #[tokio::test]
    async fn test_get_target_missing_generator_returns_none() {
        let session = GenSession::new();
        assert_eq!(session.get_target("no_such_gen", "output").await, None);
    }

    #[tokio::test]
    async fn test_get_target_missing_key_returns_none() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "output", OmniPath::new("dist/file.txt"))
            .await;
        assert_eq!(session.get_target("gen_a", "no_such_key").await, None);
    }

    #[tokio::test]
    async fn test_set_target_overwrites_existing() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "output", OmniPath::new("v1.txt"))
            .await;
        session
            .set_target("gen_a", "output", OmniPath::new("v2.txt"))
            .await;

        assert_eq!(
            session.get_target("gen_a", "output").await,
            Some(OmniPath::new("v2.txt"))
        );
    }

    #[tokio::test]
    async fn test_set_target_multiple_generators_are_isolated() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session
            .set_target("gen_b", "out", OmniPath::new("b.txt"))
            .await;

        assert_eq!(
            session.get_target("gen_a", "out").await,
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            session.get_target("gen_b", "out").await,
            Some(OmniPath::new("b.txt"))
        );
    }

    // ── set_input_raw / get_input_raw ─────────────────────────────────────────

    #[tokio::test]
    async fn test_set_get_input_raw_string() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;

        assert_eq!(
            session.get_input_raw("gen_a", "name").await,
            Some(serde_json::json!("Alice"))
        );
    }

    #[tokio::test]
    async fn test_set_get_input_raw_number() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "count", serde_json::json!(42))
            .await;

        assert_eq!(
            session.get_input_raw("gen_a", "count").await,
            Some(serde_json::json!(42))
        );
    }

    #[tokio::test]
    async fn test_set_get_input_raw_object() {
        let session = GenSession::new();
        let val = serde_json::json!({ "x": 1, "y": [true, null] });
        session.set_input_raw("gen_a", "cfg", val.clone()).await;

        assert_eq!(session.get_input_raw("gen_a", "cfg").await, Some(val));
    }

    #[tokio::test]
    async fn test_get_input_raw_missing_generator_returns_none() {
        let session = GenSession::new();
        assert_eq!(session.get_input_raw("no_gen", "key").await, None);
    }

    #[tokio::test]
    async fn test_get_input_raw_missing_key_returns_none() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;
        assert_eq!(session.get_input_raw("gen_a", "no_key").await, None);
    }

    #[tokio::test]
    async fn test_set_input_raw_overwrites_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!(1))
            .await;
        session
            .set_input_raw("gen_a", "k", serde_json::json!(2))
            .await;

        assert_eq!(
            session.get_input_raw("gen_a", "k").await,
            Some(serde_json::json!(2))
        );
    }

    // ── set_input / get_input (typed) ─────────────────────────────────────────

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestConfig {
        name: String,
        value: u32,
    }

    #[tokio::test]
    async fn test_set_get_input_typed_round_trip() {
        let session = GenSession::new();
        let cfg = TestConfig {
            name: "hello".to_string(),
            value: 99,
        };

        session.set_input("gen_a", "config", &cfg).await.unwrap();
        let got: TestConfig = session
            .get_input::<TestConfig>("gen_a", "config")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(got, cfg);
    }

    #[tokio::test]
    async fn test_get_input_missing_generator_returns_none() {
        let session = GenSession::new();
        assert!(session.get_input::<String>("no_gen", "key").await.is_none());
    }

    #[tokio::test]
    async fn test_get_input_missing_key_returns_none() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "other", serde_json::json!("x"))
            .await;
        assert!(
            session
                .get_input::<String>("gen_a", "no_such_key")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_get_input_type_mismatch_returns_err() {
        let session = GenSession::new();
        // Store a plain number; try to deserialize as a struct.
        session
            .set_input_raw("gen_a", "num", serde_json::json!(42))
            .await;

        let result = session.get_input::<TestConfig>("gen_a", "num").await;
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    // ── set_inputs / set_targets (bulk replace) ───────────────────────────────

    #[tokio::test]
    async fn test_set_inputs_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "old_key", serde_json::json!("old"))
            .await;

        let mut new_inputs = UnorderedMap::default();
        new_inputs.insert("new_key".to_string(), serde_json::json!("new_val"));
        session
            .set_inputs("gen_a", new_inputs)
            .await
            .expect("should succeed");

        assert_eq!(session.get_input_raw("gen_a", "old_key").await, None);
        assert_eq!(
            session.get_input_raw("gen_a", "new_key").await,
            Some(serde_json::json!("new_val"))
        );
    }

    #[tokio::test]
    async fn test_set_targets_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "old_out", OmniPath::new("old.txt"))
            .await;

        let mut new_targets = UnorderedMap::default();
        new_targets.insert("new_out".to_string(), OmniPath::new("new.txt"));
        session.set_targets("gen_a", new_targets).await;

        assert_eq!(session.get_target("gen_a", "old_out").await, None);
        assert_eq!(
            session.get_target("gen_a", "new_out").await,
            Some(OmniPath::new("new.txt"))
        );
    }

    // ── merge() ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_merge_combines_disjoint_generators() {
        let session = GenSession::new();
        session.set_target("a", "a", OmniPath::new("a")).await;
        session.set_target("b", "b", OmniPath::new("b")).await;

        let other = GenSession::new();
        other.set_target("a", "b", OmniPath::new("b")).await;
        other.set_target("c", "c", OmniPath::new("c")).await;

        session.merge(other).await;

        assert_eq!(
            session.get_target("a", "a").await,
            Some(OmniPath::new("a"))
        );
        assert_eq!(
            session.get_target("a", "b").await,
            Some(OmniPath::new("b"))
        );
        assert_eq!(
            session.get_target("b", "b").await,
            Some(OmniPath::new("b"))
        );
        assert_eq!(
            session.get_target("c", "c").await,
            Some(OmniPath::new("c"))
        );
    }

    #[tokio::test]
    async fn test_merge_same_key_other_wins() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("original.txt"))
            .await;

        let other = GenSession::new();
        other
            .set_target("gen_a", "out", OmniPath::new("overridden.txt"))
            .await;

        session.merge(other).await;

        assert_eq!(
            session.get_target("gen_a", "out").await,
            Some(OmniPath::new("overridden.txt"))
        );
    }

    #[tokio::test]
    async fn test_merge_inputs_are_combined() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "x", serde_json::json!(1))
            .await;

        let other = GenSession::new();
        other
            .set_input_raw("gen_a", "y", serde_json::json!(2))
            .await;

        session.merge(other).await;

        assert_eq!(
            session.get_input_raw("gen_a", "x").await,
            Some(serde_json::json!(1))
        );
        assert_eq!(
            session.get_input_raw("gen_a", "y").await,
            Some(serde_json::json!(2))
        );
    }

    #[tokio::test]
    async fn test_merge_empty_other_is_no_op() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;

        session.merge(GenSession::new()).await;

        assert_eq!(
            session.get_target("gen_a", "out").await,
            Some(OmniPath::new("a.txt"))
        );
    }

    #[tokio::test]
    async fn test_merge_into_empty_session() {
        let session = GenSession::new();

        let other = GenSession::new();
        other
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        other
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;

        session.merge(other).await;

        assert_eq!(
            session.get_target("gen_a", "out").await,
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            session.get_input_raw("gen_a", "k").await,
            Some(serde_json::json!("v"))
        );
    }

    // ── restore_targets() ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_restore_targets_fills_missing_keys() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("session.txt"))
            .await;

        let mut targets = UnorderedMap::default();
        session.restore_targets("gen_a", &mut targets, false).await;

        assert_eq!(targets.get("out"), Some(&OmniPath::new("session.txt")));
    }

    #[tokio::test]
    async fn test_restore_targets_no_override_preserves_existing() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("session.txt"))
            .await;

        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));
        session.restore_targets("gen_a", &mut targets, false).await;

        assert_eq!(targets.get("out"), Some(&OmniPath::new("existing.txt")));
    }

    #[tokio::test]
    async fn test_restore_targets_with_override_replaces_existing() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("session.txt"))
            .await;

        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));
        session.restore_targets("gen_a", &mut targets, true).await;

        assert_eq!(targets.get("out"), Some(&OmniPath::new("session.txt")));
    }

    #[tokio::test]
    async fn test_restore_targets_missing_generator_is_no_op() {
        let session = GenSession::new();
        let mut targets = UnorderedMap::default();
        targets.insert("out".to_string(), OmniPath::new("existing.txt"));

        session
            .restore_targets("no_such_gen", &mut targets, true)
            .await;

        assert_eq!(targets.get("out"), Some(&OmniPath::new("existing.txt")));
    }

    #[tokio::test]
    async fn test_restore_targets_no_override_fills_missing_leaves_others() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "from_session", OmniPath::new("s.txt"))
            .await;
        session
            .set_target("gen_a", "both", OmniPath::new("session_both.txt"))
            .await;

        let mut targets = UnorderedMap::default();
        targets.insert("both".to_string(), OmniPath::new("existing_both.txt"));

        session.restore_targets("gen_a", &mut targets, false).await;

        // Key only in session is filled in.
        assert_eq!(targets.get("from_session"), Some(&OmniPath::new("s.txt")));
        // Key present in both: existing wins (no override).
        assert_eq!(
            targets.get("both"),
            Some(&OmniPath::new("existing_both.txt"))
        );
    }

    // ── restore_inputs() ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_restore_inputs_fills_missing_keys() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;

        let mut inputs = UnorderedMap::default();
        session.restore_inputs("gen_a", &mut inputs, false).await;

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Alice")));
    }

    #[tokio::test]
    async fn test_restore_inputs_no_override_preserves_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;

        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));
        session.restore_inputs("gen_a", &mut inputs, false).await;

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Bob")));
    }

    #[tokio::test]
    async fn test_restore_inputs_with_override_replaces_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;

        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));
        session.restore_inputs("gen_a", &mut inputs, true).await;

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Alice")));
    }

    #[tokio::test]
    async fn test_restore_inputs_missing_generator_is_no_op() {
        let session = GenSession::new();
        let mut inputs = UnorderedMap::default();
        inputs.insert("name".to_string(), serde_json::json!("Bob"));

        session
            .restore_inputs("no_such_gen", &mut inputs, true)
            .await;

        assert_eq!(inputs.get("name"), Some(&serde_json::json!("Bob")));
    }

    // ── restore_inputs_as_value_bag() ─────────────────────────────────────────

    #[tokio::test]
    async fn test_restore_inputs_as_value_bag_fills_missing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "count", serde_json::json!(7))
            .await;

        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        session
            .restore_inputs_as_value_bag("gen_a", &mut map, false)
            .await;

        assert!(map.contains_key("count"));
        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(7));
    }

    #[tokio::test]
    async fn test_restore_inputs_as_value_bag_no_override_preserves_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "count", serde_json::json!(99))
            .await;

        let existing =
            ValueBag::capture_serde1(&serde_json::json!(1)).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("count".to_string(), existing);

        session
            .restore_inputs_as_value_bag("gen_a", &mut map, false)
            .await;

        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(1));
    }

    #[tokio::test]
    async fn test_restore_inputs_as_value_bag_with_override() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "count", serde_json::json!(99))
            .await;

        let existing =
            ValueBag::capture_serde1(&serde_json::json!(1)).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("count".to_string(), existing);

        session
            .restore_inputs_as_value_bag("gen_a", &mut map, true)
            .await;

        let json = serde_json::to_value(&map["count"]).unwrap();
        assert_eq!(json, serde_json::json!(99));
    }

    // ── unset_targets() ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_unset_targets_removes_specified_key() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session
            .set_target("gen_a", "other", OmniPath::new("b.txt"))
            .await;

        session.unset_targets("gen_a", ["out".to_string()]).await;

        assert_eq!(session.get_target("gen_a", "out").await, None);
        assert_eq!(
            session.get_target("gen_a", "other").await,
            Some(OmniPath::new("b.txt"))
        );
    }

    #[tokio::test]
    async fn test_unset_targets_missing_generator_does_not_panic() {
        let session = GenSession::new();
        session
            .unset_targets("no_such_gen", ["key".to_string()])
            .await;
    }

    #[tokio::test]
    async fn test_unset_targets_multiple_keys() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "a", OmniPath::new("a.txt"))
            .await;
        session
            .set_target("gen_a", "b", OmniPath::new("b.txt"))
            .await;
        session
            .set_target("gen_a", "c", OmniPath::new("c.txt"))
            .await;

        session
            .unset_targets("gen_a", ["a".to_string(), "b".to_string()])
            .await;

        assert_eq!(session.get_target("gen_a", "a").await, None);
        assert_eq!(session.get_target("gen_a", "b").await, None);
        assert_eq!(
            session.get_target("gen_a", "c").await,
            Some(OmniPath::new("c.txt"))
        );
    }

    // ── unset_inputs() ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_unset_inputs_removes_specified_key() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "x", serde_json::json!(1))
            .await;
        session
            .set_input_raw("gen_a", "y", serde_json::json!(2))
            .await;

        session.unset_inputs("gen_a", ["x".to_string()]).await;

        assert_eq!(session.get_input_raw("gen_a", "x").await, None);
        assert_eq!(
            session.get_input_raw("gen_a", "y").await,
            Some(serde_json::json!(2))
        );
    }

    #[tokio::test]
    async fn test_unset_inputs_missing_generator_does_not_panic() {
        let session = GenSession::new();
        session
            .unset_inputs("no_such_gen", ["key".to_string()])
            .await;
    }

    // ── is_empty() ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_is_empty_new_session() {
        assert!(GenSession::new().is_empty().await);
    }

    #[tokio::test]
    async fn test_is_empty_false_after_adding_target() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        assert!(!session.is_empty().await);
    }

    #[tokio::test]
    async fn test_is_empty_false_after_adding_input() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;
        assert!(!session.is_empty().await);
    }

    #[tokio::test]
    async fn test_is_empty_true_after_removing_only_target() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.unset_targets("gen_a", ["out".to_string()]).await;

        // Generator entry exists but has no targets or inputs.
        assert!(session.is_empty().await);
    }

    #[tokio::test]
    async fn test_is_empty_false_when_only_input_present() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.unset_targets("gen_a", ["out".to_string()]).await;
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;

        assert!(!session.is_empty().await);
    }

    // ── has_changes() ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_has_changes_false_when_data_matches_disk() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;
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
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session
            .set_target("gen_a", "extra", OmniPath::new("extra.txt"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_adding_input() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session
            .set_input_raw("gen_a", "k", serde_json::json!("new"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_modifying_target_value() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("original.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session
            .set_target("gen_a", "out", OmniPath::new("modified.txt"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_modifying_input_value() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!("original"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session
            .set_input_raw("gen_a", "k", serde_json::json!("changed"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_removing_target() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session.unset_targets("gen_a", ["out"]).await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_removing_input() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        session.unset_inputs("gen_a", ["k"]).await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_adding_new_generator() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        // gen_b is new – data.len() now exceeds what is on disk.
        session
            .set_target("gen_b", "out", OmniPath::new("b.txt"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_returns_err_for_missing_file() {
        let sys = InMemorySys::default();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;

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
        session
            .set_target("gen_a", "out", OmniPath::new("dist/file.txt"))
            .await;
        session
            .set_input_raw("gen_a", "name", serde_json::json!("Alice"))
            .await;

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "out").await,
            Some(OmniPath::new("dist/file.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_a", "name").await,
            Some(serde_json::json!("Alice"))
        );
    }

    #[tokio::test]
    async fn test_disk_round_trip_multiple_generators() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "a_out", OmniPath::new("a.txt"))
            .await;
        session
            .set_target("gen_b", "b_out", OmniPath::new("b.txt"))
            .await;
        session
            .set_input_raw("gen_b", "mode", serde_json::json!("fast"))
            .await;

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "a_out").await,
            Some(OmniPath::new("a.txt"))
        );
        assert_eq!(
            loaded.get_target("gen_b", "b_out").await,
            Some(OmniPath::new("b.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_b", "mode").await,
            Some(serde_json::json!("fast"))
        );
    }

    #[tokio::test]
    async fn test_has_changes_false_for_just_loaded_session() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("dist/file.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        let loaded = GenSession::from_disk(path, &sys).await.unwrap();
        assert!(!loaded.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_for_loaded_then_mutated_session() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("dist/file.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        let loaded = GenSession::from_disk(path, &sys).await.unwrap();
        loaded
            .set_target("gen_a", "new_key", OmniPath::new("other.txt"))
            .await;
        assert!(loaded.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_disk_round_trip_yaml() {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all(Path::new("/sessions"))
            .expect("create dir");
        let path = Path::new("/sessions/session.yaml");

        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("build/result.txt"))
            .await;
        session
            .set_input_raw("gen_a", "env", serde_json::json!("production"))
            .await;

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(
            loaded.get_target("gen_a", "out").await,
            Some(OmniPath::new("build/result.txt"))
        );
        assert_eq!(
            loaded.get_input_raw("gen_a", "env").await,
            Some(serde_json::json!("production"))
        );
    }

    #[tokio::test]
    async fn test_disk_round_trip_preserves_complex_inputs() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        let complex = serde_json::json!({ "list": [1, "two", null], "nested": { "ok": true } });
        session.set_input_raw("gen_a", "cfg", complex.clone()).await;

        session.write_to_disk(path, &sys).await.unwrap();
        let loaded = GenSession::from_disk(path, &sys).await.unwrap();

        assert_eq!(loaded.get_input_raw("gen_a", "cfg").await, Some(complex));
    }

    // ── set_inputs_raw() ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_inputs_raw_bulk_replaces_all_existing() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "old_key", serde_json::json!("old"))
            .await;

        let mut new_inputs = UnorderedMap::default();
        new_inputs.insert("new_key".to_string(), serde_json::json!("new_val"));
        session.set_inputs_raw("gen_a", new_inputs).await;

        assert_eq!(session.get_input_raw("gen_a", "old_key").await, None);
        assert_eq!(
            session.get_input_raw("gen_a", "new_key").await,
            Some(serde_json::json!("new_val"))
        );
    }

    #[tokio::test]
    async fn test_set_inputs_raw_creates_generator_if_absent() {
        let session = GenSession::new();
        let mut inputs = UnorderedMap::default();
        inputs.insert("k".to_string(), serde_json::json!(42));
        session.set_inputs_raw("gen_a", inputs).await;

        assert_eq!(
            session.get_input_raw("gen_a", "k").await,
            Some(serde_json::json!(42))
        );
    }

    #[tokio::test]
    async fn test_set_inputs_raw_with_empty_map_clears_inputs() {
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!(1))
            .await;
        session
            .set_inputs_raw("gen_a", UnorderedMap::default())
            .await;

        assert_eq!(session.get_input_raw("gen_a", "k").await, None);
    }

    // ── set_inputs() typed overload ───────────────────────────────────────────

    #[tokio::test]
    async fn test_set_inputs_typed_serializes_struct_values() {
        let session = GenSession::new();
        let cfg = TestConfig {
            name: "world".to_string(),
            value: 7,
        };

        let mut inputs = UnorderedMap::default();
        inputs.insert("cfg".to_string(), cfg);
        session.set_inputs("gen_a", inputs).await.unwrap();

        let got: TestConfig = session
            .get_input::<TestConfig>("gen_a", "cfg")
            .await
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

    #[tokio::test]
    async fn test_set_inputs_typed_returns_ok_for_serializable_values() {
        let session = GenSession::new();
        let mut inputs: UnorderedMap<String, serde_json::Value> =
            UnorderedMap::default();
        inputs.insert("k".to_string(), serde_json::json!(99));
        assert!(session.set_inputs("gen_a", inputs).await.is_ok());
    }

    // ── restore_inputs_as_value_bag() – missing generator ─────────────────────

    #[tokio::test]
    async fn test_restore_inputs_as_value_bag_missing_generator_is_no_op() {
        let session = GenSession::new();
        let existing =
            ValueBag::capture_serde1(&serde_json::json!("kept")).to_owned();
        let mut map: UnorderedMap<String, OwnedValueBag> =
            UnorderedMap::default();
        map.insert("k".to_string(), existing);

        session
            .restore_inputs_as_value_bag("no_such_gen", &mut map, true)
            .await;

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
        other
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.merge(other).await;

        // Disk is empty; session now has gen_a.
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_has_changes_true_after_merge_adds_new_generator() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.write_to_disk(path, &sys).await.unwrap();

        let other = GenSession::new();
        other
            .set_target("gen_b", "out", OmniPath::new("b.txt"))
            .await;
        session.merge(other).await;

        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    // ── is_empty() – multi-generator ──────────────────────────────────────────

    #[tokio::test]
    async fn test_is_empty_false_when_only_second_generator_has_data() {
        let session = GenSession::new();
        // gen_a gets a target that is then removed (empty entry remains).
        session
            .set_target("gen_a", "out", OmniPath::new("a.txt"))
            .await;
        session.unset_targets("gen_a", ["out"]).await;
        // gen_b has live data.
        session
            .set_target("gen_b", "out", OmniPath::new("b.txt"))
            .await;

        assert!(!session.is_empty().await);
    }

    #[tokio::test]
    async fn test_is_empty_true_when_all_generators_are_empty() {
        let session = GenSession::new();
        session
            .set_target("gen_a", "a", OmniPath::new("a.txt"))
            .await;
        session
            .set_target("gen_b", "b", OmniPath::new("b.txt"))
            .await;
        session.unset_targets("gen_a", ["a"]).await;
        session.unset_targets("gen_b", ["b"]).await;

        assert!(session.is_empty().await);
    }

    // ── SessionFile envelope: format compatibility and root ───────────────────

    #[tokio::test]
    async fn test_legacy_bare_map_parses_with_root_false() {
        let (sys, path) = make_sys();
        let legacy =
            br#"{ "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, legacy.to_vec()).await.unwrap();

        let file = SessionFile::load_or_default(path, &sys).await.unwrap();
        assert!(!file.root);
        assert_eq!(
            file.generators.get("gen_a").and_then(|g| g.inputs.get("k")),
            Some(&serde_json::json!("v"))
        );
    }

    #[tokio::test]
    async fn test_root_is_omitted_when_false_and_emitted_when_true() {
        let without_root = SessionFile::default();
        let json = serde_json::to_string(&without_root).unwrap();
        assert!(!json.contains("root"));

        let with_root = SessionFile {
            root: true,
            generators: UnorderedMap::default(),
        };
        let json = serde_json::to_string(&with_root).unwrap();
        assert!(json.contains("\"root\":true"));
    }

    #[tokio::test]
    async fn test_hand_written_entry_without_targets_parses() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true, "scaffold": { "inputs": { "scope": "@acme" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let file = SessionFile::load_or_default(path, &sys).await.unwrap();
        assert!(file.root);
        let entry = file.generators.get("scaffold").unwrap();
        assert!(entry.targets.is_empty());
        assert_eq!(
            entry.inputs.get("scope"),
            Some(&serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_top_level_root_key_is_not_read_as_generator() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true, "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let file = SessionFile::load_or_default(path, &sys).await.unwrap();
        assert!(file.root);
        assert_eq!(file.generators.len(), 1);
        assert!(file.generators.contains_key("gen_a"));
        assert!(!file.generators.contains_key("root"));
    }

    #[tokio::test]
    async fn test_from_disk_ignores_top_level_root_key() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true, "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::from_disk(path, &sys).await.unwrap();
        assert_eq!(
            session.get_input_raw("gen_a", "k").await,
            Some(serde_json::json!("v"))
        );
        assert_eq!(session.get_input_raw("root", "k").await, None);
    }

    #[tokio::test]
    async fn test_has_changes_reads_file_with_top_level_root_key() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true, "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::from_disk(path, &sys).await.unwrap();
        assert!(!session.has_changes(path, &sys).await.unwrap());

        session
            .set_input_raw("gen_a", "k", serde_json::json!("changed"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    // ── is_root_marked() ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_is_root_marked_true() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();
        assert!(GenSession::is_root_marked(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_root_marked_false_for_bare_file() {
        let (sys, path) = make_sys();
        let raw = br#"{ "gen_a": { "targets": {}, "inputs": {} } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();
        assert!(!GenSession::is_root_marked(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_root_marked_false_on_missing_file() {
        let sys = InMemorySys::default();
        assert!(
            !GenSession::is_root_marked(Path::new("/nope.json"), &sys)
                .await
                .unwrap()
        );
    }

    // ── retain_delta_against() ────────────────────────────────────────────────

    fn empty_set() -> UnorderedSet<String> {
        UnorderedSet::default()
    }

    #[tokio::test]
    async fn test_delta_drops_inherited_keeps_local_prunes_emptied() {
        let baseline = GenSession::new();
        baseline
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;
        baseline
            .set_input_raw("gen_b", "only", serde_json::json!("base"))
            .await;

        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;
        session
            .set_input_raw("gen_a", "name", serde_json::json!("widget"))
            .await;
        session
            .set_input_raw("gen_b", "only", serde_json::json!("base"))
            .await;

        session
            .retain_delta_against(&baseline, &empty_set(), &empty_set())
            .await;

        assert_eq!(session.get_input_raw("gen_a", "scope").await, None);
        assert_eq!(
            session.get_input_raw("gen_a", "name").await,
            Some(serde_json::json!("widget"))
        );
        assert_eq!(session.get_input_raw("gen_b", "only").await, None);
    }

    #[tokio::test]
    async fn test_delta_pins_explicit_value_even_when_equal_to_baseline() {
        let baseline = GenSession::new();
        baseline
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;

        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;

        let mut pinned_inputs = UnorderedSet::default();
        pinned_inputs.insert("scope".to_string());

        session
            .retain_delta_against(&baseline, &pinned_inputs, &empty_set())
            .await;

        assert_eq!(
            session.get_input_raw("gen_a", "scope").await,
            Some(serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_delta_removes_generator_when_all_keys_inherited() {
        let baseline = GenSession::new();
        baseline
            .set_target("gen_a", "dest", OmniPath::new("src"))
            .await;
        baseline
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;

        let session = GenSession::new();
        session
            .set_target("gen_a", "dest", OmniPath::new("src"))
            .await;
        session
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;

        session
            .retain_delta_against(&baseline, &empty_set(), &empty_set())
            .await;

        assert!(session.is_empty().await);
    }

    #[tokio::test]
    async fn test_delta_keeps_value_that_differs_even_if_textually_close() {
        let baseline = GenSession::new();
        baseline
            .set_target("gen_a", "dest", OmniPath::new("./src"))
            .await;

        let session = GenSession::new();
        session
            .set_target("gen_a", "dest", OmniPath::new("src"))
            .await;

        session
            .retain_delta_against(&baseline, &empty_set(), &empty_set())
            .await;

        assert_eq!(
            session.get_target("gen_a", "dest").await,
            Some(OmniPath::new("src"))
        );
    }

    #[tokio::test]
    async fn test_delta_keeps_generator_absent_from_baseline() {
        let baseline = GenSession::new();

        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "scope", serde_json::json!("@acme"))
            .await;

        session
            .retain_delta_against(&baseline, &empty_set(), &empty_set())
            .await;

        assert_eq!(
            session.get_input_raw("gen_a", "scope").await,
            Some(serde_json::json!("@acme"))
        );
    }

    // ── write_delta_or_prune() / delta_differs_from_disk() ────────────────────

    #[tokio::test]
    async fn test_write_delta_writes_and_creates_dir() {
        let sys = InMemorySys::default();
        let gen_dir = Path::new("/repo/pkg/.omni");
        let path = Path::new("/repo/pkg/.omni/generator.json");

        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "name", serde_json::json!("widget"))
            .await;

        assert!(session.delta_differs_from_disk(path, &sys).await.unwrap());
        let outcome = session
            .write_delta_or_prune(path, gen_dir, &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Wrote);
        assert!(sys.fs_exists_no_err_async(path).await);
    }

    #[tokio::test]
    async fn test_write_delta_unchanged_when_matching_disk() {
        let (sys, path) = make_sys();
        let raw = br#"{ "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;

        assert!(!session.delta_differs_from_disk(path, &sys).await.unwrap());
        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Unchanged);
    }

    #[tokio::test]
    async fn test_write_delta_prunes_emptied_non_root_file() {
        let (sys, path) = make_sys();
        let raw = br#"{ "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();

        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Pruned);
        assert!(!sys.fs_exists_no_err_async(path).await);
    }

    #[tokio::test]
    async fn test_write_delta_preserves_root_seal_when_generators_emptied() {
        let (sys, path) = make_sys();
        let raw = br#"{ "root": true, "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();

        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Wrote);
        assert!(sys.fs_exists_no_err_async(path).await);

        let reloaded = SessionFile::load_or_default(path, &sys).await.unwrap();
        assert!(reloaded.root);
        assert!(reloaded.generators.is_empty());
    }
}
