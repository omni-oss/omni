use std::{path::Path, sync::Mutex};

use maps::UnorderedMap;
use omni_generator_configurations::OmniPath;
use system_traits::{FsReadAsync, FsWriteAsync};
use value_bag::{OwnedValueBag, ValueBag};

#[derive(Debug, Default)]
pub struct GenSession {
    data: Mutex<DataImpl>,
}

impl GenSession {
    pub fn new(
        targets: UnorderedMap<String, OmniPath>,
        prompts: UnorderedMap<String, OwnedValueBag>,
    ) -> Self {
        Self {
            data: Mutex::new(DataImpl { targets, prompts }),
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
        let result: DataImplDeserialize =
            omni_file_data_serde::read_async(path, sys).await?;

        Ok(Self {
            data: Mutex::new(DataImpl::from_de(result)),
        })
    }
}

impl GenSession {
    pub fn add_target(
        &self,
        key: impl Into<String>,
        value: impl Into<OmniPath>,
    ) {
        self.data
            .lock()
            .unwrap()
            .targets
            .insert(key.into(), value.into());
    }

    pub fn add_prompt(
        &self,
        key: impl Into<String>,
        value: impl Into<OwnedValueBag>,
    ) {
        self.data
            .lock()
            .unwrap()
            .prompts
            .insert(key.into(), value.into());
    }

    pub fn set_prompts(&self, prompts: UnorderedMap<String, OwnedValueBag>) {
        self.data.lock().unwrap().prompts = prompts;
    }

    pub fn set_targets(&self, targets: UnorderedMap<String, OmniPath>) {
        self.data.lock().unwrap().targets = targets;
    }

    pub fn merge(&self, other: GenSession) {
        let mut data = self.data.lock().unwrap();
        let other = other.data.lock().unwrap();

        data.targets.extend(other.targets.clone());
        data.prompts.extend(other.prompts.clone());
    }

    pub fn restore_targets(
        &self,
        targets: &mut UnorderedMap<String, OmniPath>,
        override_existing: bool,
    ) {
        for (k, v) in self.data.lock().unwrap().targets.iter() {
            if override_existing || !targets.contains_key(k) {
                targets.insert(k.clone(), v.clone());
            }
        }
    }

    pub fn restore_prompts(
        &self,
        prompts: &mut UnorderedMap<String, OwnedValueBag>,
        override_existing: bool,
    ) {
        for (k, v) in self.data.lock().unwrap().prompts.iter() {
            if override_existing || !prompts.contains_key(k) {
                prompts.insert(k.clone(), v.clone());
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
