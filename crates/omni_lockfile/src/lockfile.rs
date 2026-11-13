use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use derive_new::new;
use tokio::sync::Mutex;
use tracing::Level;

use crate::{
    LockfileSys,
    error::Error,
    lockfile_data::{LockfileData, LockfileDataV1_0_0, PinLockData},
};

#[derive(new)]
pub struct Lockfile {
    data: Mutex<LockfileData>,
    is_modified: AtomicBool,
    path: PathBuf,
}

impl Lockfile {
    pub async fn load(
        file: impl Into<PathBuf>,
        sys: &impl LockfileSys,
    ) -> Result<Self, Error> {
        let file = file.into();
        if sys.fs_exists_no_err_async(file.as_path()).await {
            if !sys.fs_is_file_no_err_async(file.as_path()).await {
                return Err(eyre::eyre!(
                    "path exists but is not a file {}",
                    file.display()
                )
                .into());
            }

            let data = omni_file_data_serde::read_async::<LockfileData, _, _>(
                &file, sys,
            )
            .await?;
            Ok(Lockfile::new(
                Mutex::new(data),
                AtomicBool::new(false),
                file,
            ))
        } else {
            Ok(Lockfile::new(
                Mutex::new(LockfileData::default()),
                AtomicBool::new(false),
                file,
            ))
        }
    }
}

impl Lockfile {
    pub async fn modify(
        &self,
        updater_fn: impl FnOnce(&mut LockfileData) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut data = self.data.lock().await;
        (updater_fn)(&mut data)?;

        self.is_modified.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Lock `pin` for `(kind, locator, selector)`, replacing any earlier pin.
    #[tracing::instrument(level = Level::DEBUG, skip_all)]
    pub async fn lock_pin(
        &self,
        kind: &str,
        locator: &str,
        selector: &str,
        pin: &str,
    ) -> Result<(), Error> {
        tracing::trace!(
            "locking {kind} source: {locator}, selector: {selector}, pin: {pin}"
        );
        self.modify(|d| {
            match d {
                LockfileData::V1_0_0(v1) => {
                    v1.sources
                        .entry(kind.to_string())
                        .or_default()
                        .entry(locator.to_string())
                        .or_default()
                        .insert(selector.to_string(), PinLockData::new(pin));
                }
            }
            Ok(())
        })
        .await
    }

    /// The pin locked for `(kind, locator, selector)`, if any.
    pub async fn pin(
        &self,
        kind: &str,
        locator: &str,
        selector: &str,
    ) -> Option<String> {
        match &*self.data.lock().await {
            LockfileData::V1_0_0(v1) => v1
                .sources
                .get(kind)
                .and_then(|l| l.get(locator))
                .and_then(|s| s.get(selector))
                .map(|e| e.pin.clone()),
        }
    }

    /// Forget the pin for `(kind, locator, selector)`, dropping maps left
    /// empty.
    pub async fn remove(
        &self,
        kind: &str,
        locator: &str,
        selector: &str,
    ) -> Result<(), Error> {
        self.modify(|d| {
            match d {
                LockfileData::V1_0_0(v1) => {
                    if let Some(locators) = v1.sources.get_mut(kind) {
                        if let Some(selectors) = locators.get_mut(locator) {
                            selectors.shift_remove(selector);
                            if selectors.is_empty() {
                                locators.shift_remove(locator);
                            }
                        }
                        if locators.is_empty() {
                            v1.sources.shift_remove(kind);
                        }
                    }
                }
            }
            Ok(())
        })
        .await
    }

    /// A snapshot of every pin as `(kind, locator, selector, pin)` tuples.
    pub async fn pins(&self) -> Vec<(String, String, String, String)> {
        match &*self.data.lock().await {
            LockfileData::V1_0_0(v1) => v1
                .sources
                .iter()
                .flat_map(|(kind, locators)| {
                    locators.iter().flat_map(move |(locator, selectors)| {
                        selectors.iter().map(move |(selector, data)| {
                            (
                                kind.clone(),
                                locator.clone(),
                                selector.clone(),
                                data.pin.clone(),
                            )
                        })
                    })
                })
                .collect(),
        }
    }

    /// Every kind named in the file, including kinds with no pins.
    pub async fn kinds(&self) -> Vec<String> {
        match &*self.data.lock().await {
            LockfileData::V1_0_0(v1) => v1.sources.keys().cloned().collect(),
        }
    }

    pub async fn save(&self, sys: &impl LockfileSys) -> Result<(), Error> {
        let is_modified = self.is_modified.load(Ordering::Relaxed);
        if is_modified {
            let data = self.data.lock().await;
            let sorted = sorted_view(&data);
            omni_file_data_serde::write_async(
                self.path.as_path(),
                &sorted,
                sys,
            )
            .await?;
        }
        Ok(())
    }
}

fn sorted_view(data: &LockfileData) -> LockfileData {
    match data {
        LockfileData::V1_0_0(v1) => {
            let mut sources = v1.sources.clone();
            for locators in sources.values_mut() {
                for selectors in locators.values_mut() {
                    selectors.sort_unstable_keys();
                }
                locators.sort_unstable_keys();
            }
            sources.sort_unstable_keys();
            LockfileData::V1_0_0(LockfileDataV1_0_0 { sources })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_file_data_serde::{Format, from_slice, to_vec};

    fn data(entries: &[(&str, &str, &str, &str)]) -> LockfileData {
        let mut v1 = LockfileDataV1_0_0::default();
        for (kind, locator, selector, pin) in entries {
            v1.sources
                .entry(kind.to_string())
                .or_default()
                .entry(locator.to_string())
                .or_default()
                .insert(selector.to_string(), PinLockData::new(*pin));
        }
        LockfileData::V1_0_0(v1)
    }

    fn json(data: &LockfileData) -> String {
        String::from_utf8(to_vec(data, Format::Json).unwrap()).unwrap()
    }

    #[test]
    fn sorted_view_is_stable_across_insertion_orders() {
        let forward = data(&[
            ("git", "https://example.com/a.git", "main", "c1"),
            ("git", "https://example.com/a.git", "dev", "c2"),
            ("git", "https://example.com/b.git", "v2", "c3"),
            ("fake", "x", "s1", "p1"),
        ]);
        let reversed = data(&[
            ("fake", "x", "s1", "p1"),
            ("git", "https://example.com/b.git", "v2", "c3"),
            ("git", "https://example.com/a.git", "dev", "c2"),
            ("git", "https://example.com/a.git", "main", "c1"),
        ]);

        let a = json(&sorted_view(&forward));
        assert_eq!(a, json(&sorted_view(&reversed)));
        assert!(a.find("\"fake\"").unwrap() < a.find("\"git\"").unwrap());
        assert!(a.find("\"dev\"").unwrap() < a.find("\"main\"").unwrap());
    }

    #[test]
    fn the_new_shape_round_trips() {
        let original = data(&[("git", "https://h/r.git", "main", "c1")]);
        let back: LockfileData =
            from_slice(json(&original).as_bytes(), Format::Json).unwrap();
        assert_eq!(json(&back), json(&original));
        assert!(json(&original).contains("1.0.0"));
    }

    #[test]
    fn old_and_malformed_shapes_fail_to_load() {
        for doc in [
            // The old git-only shape.
            r#"{"version":"1.0.0","git":{"https://h/r.git":{"main":{"commit":"c1"}}}}"#,
            // An unknown entry field.
            r#"{"version":"1.0.0","sources":{"git":{"https://h/r.git":{"main":{"pin":"c1","extra":1}}}}}"#,
            // `sources` is required.
            r#"{"version":"1.0.0"}"#,
        ] {
            assert!(
                from_slice::<LockfileData>(doc.as_bytes(), Format::Json)
                    .is_err(),
                "{doc}"
            );
        }
    }
}
