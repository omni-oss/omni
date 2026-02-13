use std::{path::Path, sync::Mutex};

use maps::UnorderedMap;
use omni_generator_configurations::OmniPath;
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
        prompts: UnorderedMap<String, OwnedValueBag>,
    ) -> Self {
        Self {
            data: Mutex::new(UnorderedMap::from_iter([(
                generator_name.into(),
                DataImpl { prompts, targets },
            )])),
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
        let result: UnorderedMap<String, DataImplDeserialize> =
            omni_file_data_serde::read_async(path, sys).await?;

        Ok(Self {
            data: Mutex::new(
                result
                    .into_iter()
                    .map(|(k, v)| (k, DataImpl::from_de(v)))
                    .collect(),
            ),
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

    pub fn set_prompt(
        &self,
        generator: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<OwnedValueBag>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .prompts
            .insert(key.into(), value.into());
    }

    pub fn get_prompt(
        &self,
        generator: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> Option<OwnedValueBag> {
        self.data
            .lock()
            .unwrap()
            .get(generator.as_ref())
            .and_then(|d| d.prompts.get(key.as_ref()))
            .map(|p| p.clone())
    }

    pub fn set_prompts(
        &self,
        generator: impl Into<String>,
        prompts: UnorderedMap<String, OwnedValueBag>,
    ) {
        self.data
            .lock()
            .unwrap()
            .entry(generator.into())
            .or_insert_with(DataImpl::default)
            .prompts = prompts;
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
            data.prompts.extend(other_data.prompts.clone());
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

    pub fn restore_prompts(
        &self,
        generator: impl AsRef<str>,
        prompts: &mut UnorderedMap<String, OwnedValueBag>,
        override_existing: bool,
    ) {
        let data = self.data.lock().unwrap();
        let data = data.get(generator.as_ref()).map(|d| &d.prompts);

        if let Some(data) = data {
            for (k, v) in data {
                if override_existing || !prompts.contains_key(k) {
                    prompts.insert(k.clone(), v.clone());
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
}

#[derive(Clone, serde::Serialize, Default, Debug)]
struct DataImpl {
    targets: UnorderedMap<String, OmniPath>,
    prompts: UnorderedMap<String, OwnedValueBag>,
}

impl DataImpl {
    fn from_de(de: DataImplDeserialize) -> Self {
        Self {
            targets: de.targets,
            prompts: de
                .prompts
                .into_iter()
                .map(|(k, v)| (k, ValueBag::from_serde1(&v).to_owned()))
                .collect(),
        }
    }
}

#[derive(Clone, serde::Deserialize)]
struct DataImplDeserialize {
    targets: UnorderedMap<String, OmniPath>,
    prompts: UnorderedMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use omni_types::OmniPath;

    use super::*;

    #[tokio::test]
    async fn test_merge() {
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
}
