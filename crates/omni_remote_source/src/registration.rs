//! Workspace registrations: a small pointer file in the store that names a
//! workspace's lockfile, and so its `refs/` directory. A pruner that reads every
//! registration can see which checkouts other workspaces reference, which the
//! workspace-local reference sets alone cannot tell it (RFC 0024 F08.7).
//!
//! A registration holds no checkout list: the reference sets it points at are
//! read live, so it cannot go stale. Nothing here deletes a registration.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Directory under the store root that holds the registrations.
pub const REGISTRATIONS_SEGMENT: &str = "workspaces";

/// The registration written to `<store>/workspaces/<hash>.json`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "version", rename_all = "kebab-case")]
pub enum RegistrationData {
    #[serde(rename = "1.0.0")]
    V1_0_0(RegistrationDataV1_0_0),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistrationDataV1_0_0 {
    /// The normalized absolute path of the workspace's lockfile. Its parent's
    /// `refs/` directory holds the workspace's reference sets. Kept in the file
    /// so a hash collision can be told from a registration's own file.
    pub lockfile_path: String,
    /// Seconds since the Unix epoch when the workspace last wrote references.
    /// Informational: for listing and forgetting registrations, never used to
    /// decide what to retain.
    pub last_seen: u64,
}

impl RegistrationData {
    pub fn new(lockfile_path: String, last_seen: u64) -> Self {
        Self::V1_0_0(RegistrationDataV1_0_0 {
            lockfile_path,
            last_seen,
        })
    }

    pub fn lockfile_path(&self) -> &str {
        let Self::V1_0_0(data) = self;
        &data.lockfile_path
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, Error> {
        Ok(omni_file_data_serde::to_vec(
            self,
            omni_file_data_serde::Format::Json,
        )?)
    }

    /// Parse a registration read from `file`, which must be named after the
    /// hash of the path it stores. A file that is not is a collision or a
    /// foreign file, and is an error that names it.
    pub fn from_file_bytes(file: &Path, bytes: &[u8]) -> Result<Self, Error> {
        let data: Self = omni_file_data_serde::from_slice(
            bytes,
            omni_file_data_serde::Format::Json,
        )
        .map_err(|error| {
            Error::custom(format!(
                "cannot read the workspace registration {}: {error}",
                file.display()
            ))
        })?;
        let expected = registration_file_name(data.lockfile_path());
        if file.file_name().and_then(|name| name.to_str()) != Some(&expected) {
            return Err(Error::custom(format!(
                "the workspace registration {} does not belong to {} (expected {expected})",
                file.display(),
                data.lockfile_path()
            )));
        }
        Ok(data)
    }
}

/// The file name for a normalized lockfile path: a full-width hash, so two
/// workspaces never share a file in practice and a mismatch is detectable.
pub fn registration_file_name(lockfile_path: &str) -> String {
    format!("{}.json", blake3::hash(lockfile_path.as_bytes()).to_hex())
}

/// `path` without a Windows verbatim prefix (`\\?\C:\x` becomes `C:\x`,
/// `\\?\UNC\host\share` becomes `\\host\share`). Canonicalizing on Windows
/// returns verbatim paths; stripping them makes the same workspace hash the
/// same whether or not it was canonicalized. Other paths are unchanged.
pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match path.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\ws\lock.json"),
            r"C:\ws\lock.json"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\host\share\lock.json"),
            r"\\host\share\lock.json"
        );
        // A verbatim path that is not a drive or UNC path is left alone.
        assert_eq!(
            strip_verbatim_prefix(r"\\?\Volume{1}\x"),
            r"\\?\Volume{1}\x"
        );
        assert_eq!(
            strip_verbatim_prefix("/home/u/ws/lock.json"),
            "/home/u/ws/lock.json"
        );
    }

    #[test]
    fn a_registration_round_trips_and_names_its_file() {
        let data =
            RegistrationData::new("/ws/.omni/sources/lock.json".into(), 7);
        let bytes = data.to_vec().unwrap();
        let file = Path::new("/store/workspaces")
            .join(registration_file_name(data.lockfile_path()));
        assert_eq!(
            RegistrationData::from_file_bytes(&file, &bytes).unwrap(),
            data
        );
    }

    #[test]
    fn a_file_named_for_another_path_is_a_collision() {
        let data = RegistrationData::new("/ws/a/lock.json".into(), 1);
        let bytes = data.to_vec().unwrap();
        let other = Path::new("/store/workspaces")
            .join(registration_file_name("/ws/b/lock.json"));
        let error =
            RegistrationData::from_file_bytes(&other, &bytes).unwrap_err();
        assert!(error.to_string().contains("does not belong"), "{error}");
    }

    #[test]
    fn unknown_fields_and_versions_fail_naming_the_file() {
        let file = Path::new("/store/workspaces/x.json");
        for doc in [
            r#"{"version":"1.0.0","lockfile_path":"/a","last_seen":1,"extra":1}"#,
            r#"{"version":"2.0.0","lockfile_path":"/a","last_seen":1}"#,
            r#"{"version":"1.0.0"}"#,
            "not json",
        ] {
            let error = RegistrationData::from_file_bytes(file, doc.as_bytes())
                .unwrap_err();
            assert!(error.to_string().contains("x.json"), "{doc}: {error}");
        }
    }
}
