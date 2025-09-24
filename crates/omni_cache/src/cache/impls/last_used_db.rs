use maps::UnorderedMap;
use omni_hasher::impls::DefaultHash;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize, Default)]
struct LocalLastUsedData {
    last_used_map: UnorderedMap<
        String,
        UnorderedMap<String, UnorderedMap<DefaultHash, OffsetDateTime>>,
    >,
}

pub struct LocalLastUsedDb<'a> {
    path: &'a std::path::Path,
    data: LocalLastUsedData,
}

impl<'a> LocalLastUsedDb<'a> {
    pub async fn load(
        path: &'a std::path::Path,
    ) -> Result<Self, LocalLastUsedDbError> {
        if tokio::fs::try_exists(path).await? {
            let bytes = tokio::fs::read(path).await?;
            let (data, _): (LocalLastUsedData, usize) =
                bincode::serde::decode_from_slice(
                    &bytes,
                    bincode::config::standard(),
                )?;

            Ok(Self { path, data })
        } else {
            Ok(Self {
                path,
                data: Default::default(),
            })
        }
    }

    pub async fn update_last_used_timestamp(
        &mut self,
        project_name: &str,
        task_name: &str,
        hash: DefaultHash,
        last_used: OffsetDateTime,
    ) -> Result<(), LocalLastUsedDbError> {
        let project_map = self
            .data
            .last_used_map
            .entry(project_name.to_string())
            .or_default();

        let task_map = project_map.entry(task_name.to_string()).or_default();

        task_map.insert(hash, last_used);

        Ok(())
    }

    pub async fn get_last_used_timestamp(
        &self,
        project_name: &str,
        task_name: &str,
        hash: DefaultHash,
    ) -> Result<Option<OffsetDateTime>, LocalLastUsedDbError> {
        Ok(self
            .data
            .last_used_map
            .get(project_name)
            .and_then(|project_map| project_map.get(task_name))
            .and_then(|task_map| task_map.get(&hash))
            .copied())
    }

    pub async fn save(&self) -> Result<(), LocalLastUsedDbError> {
        let bytes = bincode::serde::encode_to_vec(
            &self.data,
            bincode::config::standard(),
        )?;
        tokio::fs::write(self.path, &bytes).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct LocalLastUsedDbError {
    kind: LocalLastUsedDbErrorKind,
    #[source]
    inner: LocalLastUsedDbErrorInner,
}

impl LocalLastUsedDbError {
    #[allow(unused)]
    pub fn kind(&self) -> LocalLastUsedDbErrorKind {
        self.kind
    }
}

impl<T: Into<LocalLastUsedDbErrorInner>> From<T> for LocalLastUsedDbError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(LocalLastUsedDbErrorKind), vis(pub), repr(u8))]
enum LocalLastUsedDbErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    BincodeEncode(#[from] bincode::error::EncodeError),
}
