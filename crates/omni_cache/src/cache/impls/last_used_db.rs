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
    sys: &'a system_traits::impls::RealSys,
    path: &'a std::path::Path,
    data: LocalLastUsedData,
}

impl<'a> LocalLastUsedDb<'a> {
    pub async fn load(
        sys: &'a system_traits::impls::RealSys,
        path: &'a std::path::Path,
    ) -> Result<Self, LocalLastUsedDbError> {
        use system_traits::FsReadAsync as _;

        if super::try_exists_following(sys, path).await? {
            let bytes = sys.fs_read_async(path).await?;
            // The file only holds an access-time hint, so a layout this build
            // cannot decode (another omni version, a torn write) is reset
            // rather than failing every lookup, stats and prune. The next
            // save replaces it.
            let data = match rmp_serde::decode::from_slice(&bytes) {
                Ok(data) => data,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "last-used timestamps are unreadable; starting empty"
                    );
                    LocalLastUsedData::default()
                }
            };

            Ok(Self { sys, path, data })
        } else {
            Ok(Self {
                sys,
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
        let bytes = rmp_serde::encode::to_vec(&self.data)?;

        crate::storage::write_atomic(self.sys, self.path, &bytes).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct LocalLastUsedDbError(pub(crate) LocalLastUsedDbErrorInner);

impl LocalLastUsedDbError {
    #[allow(unused)]
    pub fn kind(&self) -> LocalLastUsedDbErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<LocalLastUsedDbErrorInner>> From<T> for LocalLastUsedDbError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(LocalLastUsedDbErrorKind), vis(pub), repr(u8))]
pub(crate) enum LocalLastUsedDbErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    RmpSerdeEncode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    RmpSerdeDecode(#[from] rmp_serde::decode::Error),
}

#[cfg(test)]
mod tests {
    use system_traits::impls::RealSys;

    use super::*;

    #[tokio::test]
    async fn an_undecodable_file_is_a_reset_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-used-timestamps.db");
        // Another omni version's layout, or a torn write: the file only holds
        // an access-time hint, so the cache must stay usable.
        std::fs::write(&path, b"\xc1 not msgpack").unwrap();

        let mut db =
            LocalLastUsedDb::load(&RealSys, &path).await.expect("reset");
        let hash = DefaultHash::default();
        assert_eq!(
            db.get_last_used_timestamp("p", "t", hash).await.unwrap(),
            None
        );

        let now = OffsetDateTime::now_utc();
        db.update_last_used_timestamp("p", "t", hash, now)
            .await
            .unwrap();
        db.save().await.unwrap();
        let again = LocalLastUsedDb::load(&RealSys, &path).await.unwrap();
        assert_eq!(
            again.get_last_used_timestamp("p", "t", hash).await.unwrap(),
            Some(now)
        );
    }

    #[tokio::test]
    async fn a_read_error_other_than_a_bad_decode_is_still_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be cannot be read as a file.
        assert!(LocalLastUsedDb::load(&RealSys, dir.path()).await.is_err());
    }
}
