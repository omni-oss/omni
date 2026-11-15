use std::{path::Path, process::Stdio, sync::Arc, time::Duration};

use omni_remote_source::lease::SourceLease;
use omni_utils::lock::LockGuard;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(15);

async fn exclusive(root: &Path) -> Option<LockGuard> {
    SourceLease::try_acquire_exclusive(root, "git", "remote", "pin")
        .await
        .unwrap()
}

#[tokio::test]
async fn shared_leases_exclude_collectors_until_last_owner_drops() {
    let dir = tempfile::tempdir().unwrap();
    let lease = Arc::new(
        SourceLease::acquire(dir.path(), "git", "remote", "pin")
            .await
            .unwrap(),
    );
    let owner = Arc::clone(&lease);
    let independent = SourceLease::acquire(dir.path(), "git", "remote", "pin")
        .await
        .unwrap();
    assert!(exclusive(dir.path()).await.is_none());
    // A reader of one pin does not prevent collecting another.
    assert!(
        SourceLease::try_acquire_exclusive(
            dir.path(),
            "git",
            "remote",
            "other"
        )
        .await
        .unwrap()
        .is_some()
    );
    drop(lease);
    drop(independent);
    assert!(exclusive(dir.path()).await.is_none());
    drop(owner);
    let collector = exclusive(dir.path()).await.unwrap();
    assert!(exclusive(dir.path()).await.is_none());
    drop(collector);
    assert!(exclusive(dir.path()).await.is_some());
    // A leftover file is neither content nor retention state.
    let path = dir.path().join("leases/git/remote/pin.lease");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    assert!(exclusive(dir.path()).await.is_some());
    assert!(path.exists());
}

#[tokio::test]
async fn invalid_segments_and_io_errors_are_not_contention() {
    let dir = tempfile::tempdir().unwrap();
    for invalid in ["", ".", "../escape", "/absolute", "a/b", "A", "a.used"] {
        for args in [
            (invalid, "remote", "pin"),
            ("git", invalid, "pin"),
            ("git", "remote", invalid),
        ] {
            assert!(
                SourceLease::acquire(dir.path(), args.0, args.1, args.2)
                    .await
                    .is_err()
            );
            assert!(
                SourceLease::try_acquire_exclusive(
                    dir.path(),
                    args.0,
                    args.1,
                    args.2
                )
                .await
                .is_err()
            );
        }
    }
    assert!(!dir.path().join("leases").exists());
    let file = dir.path().join("file");
    std::fs::write(&file, b"not a directory").unwrap();
    assert!(
        LockGuard::try_acquire_exclusive(file.join("lock"))
            .await
            .is_err()
    );
    assert!(
        SourceLease::acquire(&file, "git", "remote", "pin")
            .await
            .is_err()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_allows_deleting_an_open_lease_file() {
    let dir = tempfile::tempdir().unwrap();
    let lease = SourceLease::acquire(dir.path(), "git", "remote", "pin")
        .await
        .unwrap();
    // std opens with delete sharing. Pin this behavior: it is why the protocol
    // must never rely on the OS rejecting lease-file deletion.
    std::fs::remove_file(dir.path().join("leases/git/remote/pin.lease"))
        .unwrap();
    drop(lease);
    assert!(exclusive(dir.path()).await.is_some());
}

async fn helper(root: &Path, mode: &str) -> Child {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lease_process_helper", "--nocapture"])
        .env("OMNI_LEASE_TEST_ROOT", root)
        .env("OMNI_LEASE_TEST_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    timeout(WAIT, async {
        loop {
            let line = lines.next_line().await.unwrap().expect("helper exited");
            if line == "READY" {
                break;
            }
        }
    })
    .await
    .expect("helper did not become ready");
    // Keep the pipe open for libtest's final result output.
    child.stdout = Some(lines.into_inner().into_inner());
    child
}

#[tokio::test]
async fn leases_coordinate_processes_and_release_on_process_death() {
    let dir = tempfile::tempdir().unwrap();
    let mut reader = helper(dir.path(), "shared").await;
    let local = SourceLease::acquire(dir.path(), "git", "remote", "pin")
        .await
        .unwrap();
    assert!(exclusive(dir.path()).await.is_none());
    drop(local);
    assert!(exclusive(dir.path()).await.is_none());
    // SIGKILL on Unix, TerminateProcess on Windows: no Rust destructors run.
    timeout(WAIT, reader.kill()).await.unwrap().unwrap();
    assert!(exclusive(dir.path()).await.is_some());
    // The child opens its own handle and observes the parent's shared lease.
    let local = SourceLease::acquire(dir.path(), "git", "remote", "pin")
        .await
        .unwrap();
    let mut probe = helper(dir.path(), "contended").await;
    drop(probe.stdin.take());
    assert!(
        timeout(WAIT, probe.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    drop(local);
}

#[tokio::test]
async fn spawned_child_does_not_extend_parent_lease() {
    let dir = tempfile::tempdir().unwrap();
    let lease = SourceLease::acquire(dir.path(), "git", "remote", "pin")
        .await
        .unwrap();
    let mut child = helper(dir.path(), "idle").await;
    drop(lease);
    // The helper is still alive, but must not have inherited the lock handle.
    assert!(child.try_wait().unwrap().is_none());
    assert!(exclusive(dir.path()).await.is_some());
    drop(child.stdin.take());
    assert!(
        timeout(WAIT, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn lease_process_helper() {
    let Some(root) = std::env::var_os("OMNI_LEASE_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let mode = std::env::var("OMNI_LEASE_TEST_MODE").unwrap();
    let _lease = match mode.as_str() {
        "shared" => Some(
            SourceLease::acquire(root, "git", "remote", "pin")
                .await
                .unwrap(),
        ),
        "contended" => {
            assert!(exclusive(root).await.is_none());
            None
        }
        "idle" => None,
        other => panic!("unknown helper mode {other}"),
    };
    use std::io::Write;
    writeln!(std::io::stdout(), "READY").unwrap();
    std::io::stdout().flush().unwrap();
    let mut byte = [0u8];
    assert_eq!(tokio::io::stdin().read(&mut byte).await.unwrap(), 0);
}
