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
    inner: Mutex<GenSessionData>,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct GenSessionData {
    generators: UnorderedMap<String, DataImpl>,
    shared: DataImpl,
}

impl GenSession {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(GenSessionData::default()),
        }
    }

    pub fn with_restored(
        generator_name: impl Into<String>,
        targets: UnorderedMap<String, OmniPath>,
        inputs: UnorderedMap<String, serde_json::Value>,
    ) -> Self {
        let generators = UnorderedMap::from_iter([(
            generator_name.into(),
            DataImpl { inputs, targets },
        )]);
        Self {
            inner: Mutex::new(GenSessionData {
                generators,
                shared: DataImpl::default(),
            }),
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
        let SessionFileV1_0_0 {
            generators, shared, ..
        } = result.into_v1();

        Ok(Self {
            inner: Mutex::new(GenSessionData { generators, shared }),
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
        self.inner
            .lock()
            .await
            .generators
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
        self.inner
            .lock()
            .await
            .generators
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
        self.inner
            .lock()
            .await
            .generators
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
        self.inner
            .lock()
            .await
            .generators
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
        self.inner
            .lock()
            .await
            .generators
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .cloned()
    }

    pub async fn get_input<T: serde::de::DeserializeOwned>(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<Result<T, serde_json::Error>> {
        self.inner
            .lock()
            .await
            .generators
            .get(generator.as_ref())
            .and_then(|d| d.inputs.get(key.as_ref()))
            .map(|p| serde_json::from_value(p.clone()))
    }

    pub async fn set_inputs_raw(
        &self,
        generator: impl Into<String>,
        inputs: UnorderedMap<String, serde_json::Value>,
    ) {
        self.inner
            .lock()
            .await
            .generators
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
        self.inner
            .lock()
            .await
            .generators
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .targets = targets;
    }

    pub async fn merge(&self, other: GenSession) {
        let mut inner = self.inner.lock().await;
        let other = other.inner.lock().await;

        for (generator, other_data) in other.generators.iter() {
            let data = inner
                .generators
                .entry(generator.clone())
                .or_insert_with(DataImpl::default);
            data.targets.extend(other_data.targets.clone());
            data.inputs.extend(other_data.inputs.clone());
        }
    }

    /// Collapse the shared block and this generator's specific entry into one
    /// `DataImpl`, with the generator-specific values overriding shared.
    pub async fn resolved_for(&self, generator: impl AsRef<str>) -> DataImpl {
        let inner = self.inner.lock().await;
        let mut resolved = inner.shared.clone();
        if let Some(specific) = inner.generators.get(generator.as_ref()) {
            resolved.targets.extend(specific.targets.clone());
            resolved.inputs.extend(specific.inputs.clone());
        }
        resolved
    }

    /// Clone the shared block on its own (used to seed the baseline with the
    /// output directory file's own shared values).
    pub async fn shared_dataimpl(&self) -> DataImpl {
        self.inner.lock().await.shared.clone()
    }

    /// Fold a resolved `DataImpl` into this generator's entry, with the incoming
    /// (deeper) values overriding what is already present.
    pub async fn overlay_generator(
        &self,
        generator: impl Into<String>,
        data: DataImpl,
    ) {
        let mut inner = self.inner.lock().await;
        let entry = inner
            .generators
            .entry(generator.into())
            .or_insert_with(DataImpl::default);
        entry.targets.extend(data.targets);
        entry.inputs.extend(data.inputs);
    }

    pub async fn set_shared_target(
        &self,
        key: impl Into<String>,
        value: impl Into<OmniPath>,
    ) {
        self.inner
            .lock()
            .await
            .shared
            .targets
            .insert(key.into(), value.into());
    }

    pub async fn set_shared_input_raw(
        &self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) {
        self.inner
            .lock()
            .await
            .shared
            .inputs
            .insert(key.into(), value.into());
    }

    pub async fn get_shared_target(
        &self,
        key: impl AsRef<str>,
    ) -> Option<OmniPath> {
        self.inner
            .lock()
            .await
            .shared
            .targets
            .get(key.as_ref())
            .cloned()
    }

    pub async fn get_shared_input_raw(
        &self,
        key: impl AsRef<str>,
    ) -> Option<serde_json::Value> {
        self.inner
            .lock()
            .await
            .shared
            .inputs
            .get(key.as_ref())
            .cloned()
    }

    pub async fn restore_targets(
        &self,
        generator: impl AsRef<str>,
        targets: &mut UnorderedMap<String, OmniPath>,
        override_existing: bool,
    ) {
        let inner = self.inner.lock().await;
        let data = inner.generators.get(generator.as_ref()).map(|d| &d.targets);

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
        let inner = self.inner.lock().await;
        let data = inner.generators.get(generator.as_ref()).map(|d| &d.inputs);

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
        let inner = self.inner.lock().await;
        let data = inner.generators.get(generator.as_ref()).map(|d| &d.inputs);

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
        let inner = self.inner.lock().await;
        let out = SessionFile::V1_0_0(SessionFileV1_0_0 {
            root: false,
            generators: inner.generators.clone(),
            shared: inner.shared.clone(),
        });
        drop(inner);
        omni_file_data_serde::write_async(path, &out, sys).await?;

        Ok(())
    }

    pub async fn unset_targets(
        &self,
        generator: impl Into<String>,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        let mut inner = self.inner.lock().await;
        if let Some(data) = inner.generators.get_mut(generator.into().as_str())
        {
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
        let mut inner = self.inner.lock().await;
        if let Some(data) = inner.generators.get_mut(generator.into().as_str())
        {
            for key in keys {
                data.inputs.remove(key.as_ref());
            }
        }
    }

    pub async fn is_empty(&self) -> bool {
        let inner = self.inner.lock().await;
        if inner.generators.is_empty() {
            return true;
        }

        for data in inner.generators.values() {
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
        let inner = self.inner.lock().await;
        let original: SessionFile =
            omni_file_data_serde::read_async(serialized_file_path, sys).await?;
        let original = original.into_v1().generators;

        if inner.generators.len() != original.len() {
            return Ok(true);
        }

        for (generator, data) in inner.generators.iter() {
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
        Ok(SessionFile::load_or_default(path, sys)
            .await?
            .into_v1()
            .root)
    }

    pub async fn retain_delta_against(
        &self,
        baseline: &GenSession,
        pinned_inputs: &UnorderedSet<String>,
        pinned_targets: &UnorderedSet<String>,
    ) {
        let mut inner = self.inner.lock().await;
        let baseline = baseline.inner.lock().await;

        let generator_names: Vec<String> =
            inner.generators.keys().cloned().collect();

        for generator in generator_names {
            let Some(base) = baseline.generators.get(&generator) else {
                continue;
            };

            let entry = inner
                .generators
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
                inner.generators.remove(&generator);
            }
        }

        // An entry with no targets and no inputs carries nothing to persist,
        // even when the generator was absent from the baseline. Drop it so the
        // delta is truly empty and the save gate does not treat a data-less run
        // as a change.
        inner.generators.retain(|_, entry| {
            !(entry.targets.is_empty() && entry.inputs.is_empty())
        });
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
        let existing =
            SessionFile::load_or_default(&path, sys).await?.into_v1();
        let inner = self.inner.lock().await;
        Ok(inner.generators != existing.generators)
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
        let existing = SessionFile::load_or_default(path, sys).await?.into_v1();
        let inner = self.inner.lock().await;

        if inner.generators == existing.generators {
            return Ok(DeltaSaveOutcome::Unchanged);
        }

        let is_empty = inner
            .generators
            .values()
            .all(|d| d.targets.is_empty() && d.inputs.is_empty());

        if is_empty && existing.shared.is_empty() && !existing.root {
            if sys.fs_exists_no_err_async(path).await {
                sys.fs_remove_file_async(path).await?;
                return Ok(DeltaSaveOutcome::Pruned);
            }
            return Ok(DeltaSaveOutcome::Unchanged);
        }

        let out = SessionFile::V1_0_0(SessionFileV1_0_0 {
            root: existing.root,
            generators: inner.generators.clone(),
            shared: existing.shared,
        });
        drop(inner);

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
pub struct DataImpl {
    #[serde(default)]
    targets: UnorderedMap<String, OmniPath>,
    #[serde(default, alias = "prompts")]
    inputs: UnorderedMap<String, serde_json::Value>,
}

impl DataImpl {
    fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.inputs.is_empty()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn map_is_empty<K, V>(map: &UnorderedMap<K, V>) -> bool {
    map.is_empty()
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "version", rename_all = "kebab-case")]
enum SessionFile {
    #[serde(rename = "1.0.0")]
    V1_0_0(SessionFileV1_0_0),
}

impl Default for SessionFile {
    fn default() -> Self {
        Self::V1_0_0(SessionFileV1_0_0::default())
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq,
)]
struct SessionFileV1_0_0 {
    #[serde(default, skip_serializing_if = "is_false")]
    root: bool,
    #[serde(default, skip_serializing_if = "map_is_empty")]
    generators: UnorderedMap<String, DataImpl>,
    #[serde(default, skip_serializing_if = "DataImpl::is_empty")]
    shared: DataImpl,
}

impl SessionFile {
    fn into_v1(self) -> SessionFileV1_0_0 {
        match self {
            SessionFile::V1_0_0(v) => v,
        }
    }

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

    // ── SessionFile envelope: versioned format, root, shared ─────────────────

    #[tokio::test]
    async fn test_legacy_bare_map_fails_to_parse() {
        let (sys, path) = make_sys();
        let legacy =
            br#"{ "gen_a": { "targets": {}, "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, legacy.to_vec()).await.unwrap();

        assert!(SessionFile::load_or_default(path, &sys).await.is_err());
        assert!(GenSession::from_disk(path, &sys).await.is_err());
    }

    #[tokio::test]
    async fn test_rev1_flattened_file_fails_to_parse() {
        let (sys, path) = make_sys();
        let rev1 = br#"{ "root": true, "gen_a": { "inputs": { "k": "v" } } }"#;
        sys.fs_write_async(path, rev1.to_vec()).await.unwrap();

        assert!(SessionFile::load_or_default(path, &sys).await.is_err());
    }

    #[tokio::test]
    async fn test_empty_envelope_serializes_with_version_only() {
        let json = serde_json::to_string(&SessionFile::default()).unwrap();
        assert_eq!(json, r#"{"version":"1.0.0"}"#);
    }

    #[tokio::test]
    async fn test_versioned_envelope_round_trip() {
        let full = SessionFile::V1_0_0(SessionFileV1_0_0 {
            root: true,
            generators: UnorderedMap::from_iter([(
                "react-lib".to_string(),
                DataImpl {
                    targets: UnorderedMap::default(),
                    inputs: UnorderedMap::from_iter([(
                        "name".to_string(),
                        serde_json::json!("widgets"),
                    )]),
                },
            )]),
            shared: DataImpl {
                targets: UnorderedMap::default(),
                inputs: UnorderedMap::from_iter([(
                    "scope".to_string(),
                    serde_json::json!("@acme"),
                )]),
            },
        });

        let json = serde_json::to_string(&full).unwrap();
        assert!(json.contains(r#""version":"1.0.0""#));

        let back: SessionFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, full);
    }

    #[tokio::test]
    async fn test_root_is_omitted_when_false_and_emitted_when_true() {
        let without_root = SessionFile::default();
        let json = serde_json::to_string(&without_root).unwrap();
        assert!(!json.contains("root"));

        let with_root = SessionFile::V1_0_0(SessionFileV1_0_0 {
            root: true,
            generators: UnorderedMap::default(),
            shared: DataImpl::default(),
        });
        let json = serde_json::to_string(&with_root).unwrap();
        assert!(json.contains(r#""root":true"#));
    }

    #[tokio::test]
    async fn test_hand_written_entry_without_targets_parses() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "root": true, "generators": { "scaffold": { "inputs": { "scope": "@acme" } } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let file = SessionFile::load_or_default(path, &sys)
            .await
            .unwrap()
            .into_v1();
        assert!(file.root);
        let entry = file.generators.get("scaffold").unwrap();
        assert!(entry.targets.is_empty());
        assert_eq!(
            entry.inputs.get("scope"),
            Some(&serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_shared_block_parses_and_round_trips() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "generators": { "gen_a": { "inputs": { "name": "widget" } } }, "shared": { "inputs": { "scope": "@acme" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::from_disk(path, &sys).await.unwrap();
        assert_eq!(
            session.get_input_raw("gen_a", "name").await,
            Some(serde_json::json!("widget"))
        );
        assert_eq!(
            session.get_shared_input_raw("scope").await,
            Some(serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_generator_named_like_reserved_keys_round_trips() {
        let (sys, path) = make_sys();
        let session = GenSession::new();
        for name in ["root", "version", "shared", "generators"] {
            session
                .set_input_raw(name, "k", serde_json::json!(name))
                .await;
        }
        session.write_to_disk(path, &sys).await.unwrap();

        let reloaded = GenSession::from_disk(path, &sys).await.unwrap();
        for name in ["root", "version", "shared", "generators"] {
            assert_eq!(
                reloaded.get_input_raw(name, "k").await,
                Some(serde_json::json!(name))
            );
        }
    }

    #[tokio::test]
    async fn test_has_changes_reads_versioned_file() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "root": true, "generators": { "gen_a": { "inputs": { "k": "v" } } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::from_disk(path, &sys).await.unwrap();
        assert!(!session.has_changes(path, &sys).await.unwrap());

        session
            .set_input_raw("gen_a", "k", serde_json::json!("changed"))
            .await;
        assert!(session.has_changes(path, &sys).await.unwrap());
    }

    // ── resolved_for() / overlay_generator() / cascade ────────────────────────

    #[tokio::test]
    async fn test_resolved_for_shared_only() {
        let session = GenSession::new();
        session
            .set_shared_input_raw("scope", serde_json::json!("@acme"))
            .await;

        let resolved = session.resolved_for("gen_a").await;
        assert_eq!(
            resolved.inputs.get("scope"),
            Some(&serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_resolved_for_specific_overrides_shared_within_file() {
        let session = GenSession::new();
        session
            .set_shared_input_raw("color", serde_json::json!("shared"))
            .await;
        session
            .set_input_raw("gen_a", "color", serde_json::json!("specific"))
            .await;

        let resolved = session.resolved_for("gen_a").await;
        assert_eq!(
            resolved.inputs.get("color"),
            Some(&serde_json::json!("specific"))
        );
    }

    #[tokio::test]
    async fn test_resolved_for_no_data_is_empty() {
        let session = GenSession::new();
        assert!(session.resolved_for("gen_a").await.is_empty());
    }

    #[tokio::test]
    async fn test_cascade_nearer_shared_beats_farther_specific() {
        let farther = GenSession::new();
        farther
            .set_input_raw(
                "gen_a",
                "color",
                serde_json::json!("farther-specific"),
            )
            .await;

        let nearer = GenSession::new();
        nearer
            .set_shared_input_raw("color", serde_json::json!("nearer-shared"))
            .await;

        let effective = GenSession::new();
        effective
            .overlay_generator("gen_a", farther.resolved_for("gen_a").await)
            .await;
        effective
            .overlay_generator("gen_a", nearer.resolved_for("gen_a").await)
            .await;

        assert_eq!(
            effective.get_input_raw("gen_a", "color").await,
            Some(serde_json::json!("nearer-shared"))
        );
    }

    #[tokio::test]
    async fn test_root_and_shared_survive_delta_save() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "root": true, "generators": { "gen_a": { "inputs": { "k": "v" } } }, "shared": { "inputs": { "scope": "@acme" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();
        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Wrote);

        let reloaded = SessionFile::load_or_default(path, &sys)
            .await
            .unwrap()
            .into_v1();
        assert!(reloaded.root);
        assert!(reloaded.generators.is_empty());
        assert_eq!(
            reloaded.shared.inputs.get("scope"),
            Some(&serde_json::json!("@acme"))
        );
    }

    #[tokio::test]
    async fn test_prune_retains_shared_only_file() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "generators": { "gen_a": { "inputs": { "k": "v" } } }, "shared": { "inputs": { "scope": "@acme" } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();
        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Wrote);
        assert!(sys.fs_exists_no_err_async(path).await);

        let reloaded = SessionFile::load_or_default(path, &sys)
            .await
            .unwrap()
            .into_v1();
        assert!(reloaded.generators.is_empty());
        assert_eq!(
            reloaded.shared.inputs.get("scope"),
            Some(&serde_json::json!("@acme"))
        );
    }

    // ── is_root_marked() ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_is_root_marked_true() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "root": true }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();
        assert!(GenSession::is_root_marked(path, &sys).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_root_marked_false_for_bare_file() {
        let (sys, path) = make_sys();
        let raw = br#"{ "version": "1.0.0", "generators": { "gen_a": { "inputs": {} } } }"#;
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

    #[tokio::test]
    async fn test_delta_drops_empty_entry_absent_from_baseline() {
        let sys = InMemorySys::default();
        let path = Path::new("/repo/.omni/generator.json");

        let baseline = GenSession::new();
        let session = GenSession::new();
        // A generator that produced no remembered inputs and no targets leaves
        // an empty entry behind.
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;
        session.unset_inputs("gen_a", ["k"]).await;

        session
            .retain_delta_against(&baseline, &empty_set(), &empty_set())
            .await;

        // The empty entry is dropped, so an absent file is not a difference and
        // nothing is written or pruned.
        assert!(!session.delta_differs_from_disk(path, &sys).await.unwrap());
        let outcome = session
            .write_delta_or_prune(path, Path::new("/repo/.omni"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Unchanged);
        assert!(!sys.fs_exists_no_err_async(path).await);
    }

    #[tokio::test]
    async fn test_write_delta_prune_of_absent_file_is_unchanged() {
        let sys = InMemorySys::default();
        let path = Path::new("/repo/.omni/generator.json");

        // An effectively-empty session (empty entry) against a file that does
        // not exist must not error trying to remove a missing file.
        let session = GenSession::new();
        session
            .set_input_raw("gen_a", "k", serde_json::json!("v"))
            .await;
        session.unset_inputs("gen_a", ["k"]).await;

        let outcome = session
            .write_delta_or_prune(path, Path::new("/repo/.omni"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Unchanged);
        assert!(!sys.fs_exists_no_err_async(path).await);
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
        let raw = br#"{ "version": "1.0.0", "generators": { "gen_a": { "inputs": { "k": "v" } } } }"#;
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
        let raw = br#"{ "version": "1.0.0", "generators": { "gen_a": { "inputs": { "k": "v" } } } }"#;
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
        let raw = br#"{ "version": "1.0.0", "root": true, "generators": { "gen_a": { "inputs": { "k": "v" } } } }"#;
        sys.fs_write_async(path, raw.to_vec()).await.unwrap();

        let session = GenSession::new();

        let outcome = session
            .write_delta_or_prune(path, Path::new("/sessions"), &sys)
            .await
            .unwrap();
        assert_eq!(outcome, DeltaSaveOutcome::Wrote);
        assert!(sys.fs_exists_no_err_async(path).await);

        let reloaded = SessionFile::load_or_default(path, &sys)
            .await
            .unwrap()
            .into_v1();
        assert!(reloaded.root);
        assert!(reloaded.generators.is_empty());
    }
}
