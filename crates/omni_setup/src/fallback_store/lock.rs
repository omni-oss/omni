use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_millis(10);

fn lock_path(keystore: &Path) -> PathBuf {
    let mut name = keystore.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    keystore.with_file_name(name)
}

/// Takes an exclusive advisory lock on a sibling file of the keystore.
///
/// The lock lives on its own file because publishing replaces the keystore
/// by rename. The returned file releases the lock when dropped (or when the
/// process dies). Gives up with `TimedOut` instead of waiting forever.
pub(super) fn acquire(keystore: &Path, timeout: Duration) -> io::Result<File> {
    let path = lock_path(keystore);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for the keystore lock",
                    ));
                }
                std::thread::sleep(RETRY_INTERVAL);
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contended_lock_times_out_and_is_reusable_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let keystore = dir.path().join("sub").join("keystore");
        let held = acquire(&keystore, Duration::from_secs(1)).unwrap();
        let error = acquire(&keystore, Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(held);
        acquire(&keystore, Duration::from_secs(1)).unwrap();
    }
}
