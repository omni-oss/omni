use std::{
    borrow::Cow,
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use omni_capabilities::{CapabilityRules, PathRoots, Root};
use omni_capability_sys::{
    CapabilityAuthorizer, EvaluatingAuthorizer, PolicyEnforcingSys,
};
use omni_contained_read::{
    ContainedReader, PlainReadThrough, ReadThrough, RealRead, SharedAuthorizer,
    input_limits,
};
use omni_containment_configurations::FsReadLimits;
use omni_test_capabilities::Capability;
use system_traits::{BaseFsReadAsync, impls::RealSys};

fn rules(json: &str) -> CapabilityRules {
    serde_json::from_str(json).expect("valid capability chain")
}

fn authorizer(
    root: &Path,
    json: &str,
) -> SharedAuthorizer<EvaluatingAuthorizer<(), Root>> {
    SharedAuthorizer::new(EvaluatingAuthorizer::new(
        rules(json),
        PathRoots::new().with(Root::Workspace, root),
        (),
    ))
}

const ALLOW_WORKSPACE: &str =
    r#"[{"access":"allow","domain":"fs.read","patterns":["@workspace/**"]}]"#;
const DENY_SECRET: &str = r#"[
    {"access":"allow","domain":"fs.read","patterns":["@workspace/**"]},
    {"access":"deny","domain":"fs.read","patterns":["@workspace/secret/**"]}
]"#;

fn at(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |p, s| p.join(s))
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(at(&root, "a/b")).unwrap();
        fs::create_dir_all(at(&root, "secret")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(at(&root, "a/b/file"), "inside").unwrap();
        fs::write(at(&root, "a/other"), "other").unwrap();
        fs::write(at(&root, "secret/key"), "SECRET").unwrap();
        fs::write(at(&outside, "secret"), "OUTSIDE").unwrap();
        Self {
            _tmp: tmp,
            root,
            outside,
        }
    }
}

/// Counts what the seam is asked, serves an overlay first, and resolves
/// relative paths against `cwd`.
struct Overlay {
    cwd: PathBuf,
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    consulted: AtomicUsize,
}

impl Overlay {
    fn new(cwd: &Path) -> Arc<Self> {
        Arc::new(Self {
            cwd: cwd.to_path_buf(),
            files: Mutex::default(),
            consulted: AtomicUsize::new(0),
        })
    }

    fn put(&self, path: PathBuf, bytes: &[u8]) {
        self.files.lock().unwrap().insert(path, bytes.to_vec());
    }
}

struct Seam(Arc<Overlay>);

#[async_trait]
impl ReadThrough for Seam {
    async fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.0.cwd.join(path)
        })
    }

    async fn read_through(
        &self,
        path: &Path,
        real: &dyn RealRead,
    ) -> io::Result<Cow<'static, [u8]>> {
        self.0.consulted.fetch_add(1, Ordering::SeqCst);
        if let Some(bytes) = self.0.files.lock().unwrap().get(path) {
            return Ok(Cow::Owned(bytes.clone()));
        }
        real.read(path).await
    }
}

/// Stands in for the broker: records the paths it serves.
#[derive(Default)]
struct Broker {
    served: Mutex<Vec<PathBuf>>,
}

struct BrokerHandle(Arc<Broker>);

#[async_trait]
impl BaseFsReadAsync for BrokerHandle {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        self.0.served.lock().unwrap().push(path.to_path_buf());
        Ok(Cow::Borrowed(b"delegated"))
    }
}

struct Harness {
    fx: Fixture,
    overlay: Arc<Overlay>,
    broker: Arc<Broker>,
    reader: ContainedReader<BrokerHandle, Seam>,
}

fn harness_with(json: &str, limits: FsReadLimits, with_root: bool) -> Harness {
    let fx = Fixture::new();
    let overlay = Overlay::new(&fx.root);
    let broker = Arc::new(Broker::default());
    let reader = ContainedReader::new(
        BrokerHandle(broker.clone()),
        Seam(overlay.clone()),
        authorizer(&fx.root, json),
        with_root.then(|| fx.root.clone()),
        input_limits(&limits),
    );
    Harness {
        fx,
        overlay,
        broker,
        reader,
    }
}

fn harness() -> Harness {
    harness_with(ALLOW_WORKSPACE, FsReadLimits::DEFAULT, true)
}

/// Where the platform cannot contain (macOS by design, a Linux sandbox that
/// blocks `openat2`), the read must be `Unsupported` and read nothing; the rest
/// of the test is skipped with a named gate. x86_64 Linux and Windows must
/// support it.
async fn supported(h: &Harness) -> bool {
    let probe = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "a/b/file"))
        .await;
    match probe {
        Ok(bytes) => {
            assert_eq!(&*bytes, b"inside");
            true
        }
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            assert!(
                error
                    .to_string()
                    .starts_with("input_view.containment_unsupported"),
                "{error}"
            );
            let by_design = cfg!(target_os = "macos")
                || (cfg!(target_os = "linux")
                    && (!cfg!(target_arch = "x86_64")
                        || std::env::var_os(
                            "OMNI_INPUT_VIEW_ALLOW_NO_OPENAT2",
                        )
                        .is_some()));
            if !by_design {
                panic!("this target must not answer Unsupported: {error}");
            }
            let _ = omni_test_capabilities::gate(
                "openat2",
                false,
                "containment is unsupported on this target",
            );
            false
        }
        Err(error) => panic!("unexpected probe result: {error}"),
    }
}

#[tokio::test]
async fn an_in_root_read_returns_the_bytes_and_a_missing_file_is_not_found() {
    let h = harness();
    if !supported(&h).await {
        return;
    }
    let bytes = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "a/other"))
        .await
        .unwrap();
    assert_eq!(&*bytes, b"other");
    let missing = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "a/none"))
        .await
        .unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    assert!(h.broker.served.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_denied_path_is_refused_before_the_seam_or_the_disk() {
    let h = harness_with(DENY_SECRET, FsReadLimits::DEFAULT, true);
    let before = h.overlay.consulted.load(Ordering::SeqCst);
    let error = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "secret/key"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        error.to_string().starts_with("input_view.denied"),
        "{error}"
    );
    assert!(!error.to_string().contains("SECRET"));
    assert_eq!(h.overlay.consulted.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn a_denied_path_with_an_overlay_entry_is_still_denied() {
    let h = harness_with(DENY_SECRET, FsReadLimits::DEFAULT, true);
    let path = at(&h.fx.root, "secret/key");
    h.overlay.put(path.clone(), b"from the overlay");
    let error = h.reader.base_fs_read_async(&path).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(h.overlay.consulted.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_allowed_overlay_entry_is_served_without_touching_the_disk() {
    let h = harness();
    let path = at(&h.fx.root, "a/new-file");
    h.overlay.put(path.clone(), b"uncommitted");
    // The file does not exist on disk; only the overlay has it.
    let bytes = h.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*bytes, b"uncommitted");
}

#[tokio::test]
async fn dotdot_inside_the_root_is_cleaned_and_contained() {
    let h = harness();
    if !supported(&h).await {
        return;
    }
    let spelled = h.fx.root.join("a").join("b").join("..").join("other");
    let bytes = h.reader.base_fs_read_async(&spelled).await.unwrap();
    assert_eq!(&*bytes, b"other");
    // `..` back out of the root is outside, so it is the broker's.
    let out = h.fx.root.join("..").join("outside").join("secret");
    let bytes = h.reader.base_fs_read_async(&out).await.unwrap();
    assert_eq!(&*bytes, b"delegated");
    assert_eq!(h.broker.served.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_relative_path_is_resolved_by_the_seam_before_routing() {
    let h = harness();
    if !supported(&h).await {
        return;
    }
    // `cwd` of the seam is the workspace root.
    let bytes = h
        .reader
        .base_fs_read_async(&at(Path::new(""), "a/other"))
        .await
        .unwrap();
    assert_eq!(&*bytes, b"other");
}

#[tokio::test]
async fn a_path_wholly_outside_the_root_is_delegated_unchanged() {
    let h = harness();
    let target = at(&h.fx.outside, "secret");
    let bytes = h.reader.base_fs_read_async(&target).await.unwrap();
    assert_eq!(&*bytes, b"delegated");
    assert_eq!(*h.broker.served.lock().unwrap(), vec![target]);
}

#[tokio::test]
async fn without_a_workspace_root_every_read_fails_closed() {
    let h = harness_with(ALLOW_WORKSPACE, FsReadLimits::DEFAULT, false);
    let error = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "a/other"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let error = h
        .reader
        .base_fs_read_async(&at(&h.fx.outside, "secret"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(h.broker.served.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_oversized_file_is_refused_before_it_is_allocated() {
    let limits = FsReadLimits {
        max_file_bytes: 4,
        max_total_bytes: 4,
        ..FsReadLimits::DEFAULT
    };
    let h = harness_with(ALLOW_WORKSPACE, limits, true);
    // `inside` is 6 bytes.
    let error = h
        .reader
        .base_fs_read_async(&at(&h.fx.root, "a/b/file"))
        .await
        .unwrap_err();
    if error.kind() == io::ErrorKind::Unsupported {
        let _ = supported(&h).await;
        return;
    }
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().starts_with("input_view.too_large"),
        "{error}"
    );
}

#[tokio::test]
async fn errors_never_show_the_absolute_root() {
    let h = harness_with(DENY_SECRET, FsReadLimits::DEFAULT, true);
    let root = h.fx.root.to_string_lossy().to_string();
    for path in [
        at(&h.fx.root, "secret/key"),
        at(&h.fx.root, "a/b/file").join("x\0y"),
    ] {
        if let Err(error) = h.reader.base_fs_read_async(&path).await {
            assert!(!error.to_string().contains(&root), "{error}");
        }
    }
}

mod symlinks {
    use super::*;

    #[cfg(unix)]
    fn link(target: &Path, at: &Path) {
        std::os::unix::fs::symlink(target, at).unwrap();
    }

    #[cfg(windows)]
    fn link(target: &Path, at: &Path) {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, at).unwrap();
        } else {
            std::os::windows::fs::symlink_file(target, at).unwrap();
        }
    }

    fn allowed(h: &Harness) -> bool {
        omni_test_capabilities::require(Capability::Symlink, &h.fx.root)
    }

    fn assert_refused(error: io::Error) {
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        let text = error.to_string();
        assert!(
            text.starts_with("input_view.symlink")
                || text.starts_with("input_view.reparse_point"),
            "{text}"
        );
        assert!(!text.contains("OUTSIDE"));
    }

    #[tokio::test]
    async fn symlinks_are_rejected_in_every_shape() {
        let h = harness();
        if !allowed(&h) || !supported(&h).await {
            return;
        }
        let root = &h.fx.root;
        link(&at(&h.fx.outside, "secret"), &at(root, "to-outside"));
        link(&at(root, "a/other"), &at(root, "to-inside"));
        link(&at(root, "a/b"), &at(root, "dir-link"));
        link(&at(root, "does-not-exist"), &at(root, "dangling"));
        for relative in ["to-outside", "to-inside", "dir-link/file", "dangling"]
        {
            let error = h
                .reader
                .base_fs_read_async(&at(root, relative))
                .await
                .unwrap_err();
            assert_refused(error);
        }
        // the same name as a real file reads normally
        let bytes = h
            .reader
            .base_fs_read_async(&at(root, "a/b/file"))
            .await
            .unwrap();
        assert_eq!(&*bytes, b"inside");
    }

    #[tokio::test]
    async fn an_alias_of_the_root_is_refused_not_delegated() {
        let h = harness();
        if !allowed(&h) || !supported(&h).await {
            return;
        }
        let alias = h.fx.root.parent().unwrap().join("alias-of-root");
        link(&h.fx.root, &alias);
        let error = h
            .reader
            .base_fs_read_async(&at(&alias, "a/b/file"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert!(error.to_string().contains("alias"), "{error}");
        assert!(h.broker.served.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_file_is_swapped_for_a_symlink_right_before_the_open() {
        // Deterministic variant: the authorizer runs right before the view
        // opens the file, so it swaps the file for a link to an outside file at
        // exactly that point. No outside byte may come back.
        let fx = Fixture::new();
        if !omni_test_capabilities::require(Capability::Symlink, &fx.root) {
            return;
        }
        struct Swap {
            inner: SharedAuthorizer<EvaluatingAuthorizer<(), Root>>,
            victim: PathBuf,
            outside: PathBuf,
            calls: AtomicUsize,
        }
        impl CapabilityAuthorizer for Swap {
            fn authorize(
                &self,
                request: &omni_capabilities::Request<'_>,
            ) -> omni_capabilities::Decision {
                let decision = self.inner.authorize(request);
                if let omni_capabilities::Request::Fs { path, .. } = request
                    && *path == self.victim.as_path()
                {
                    // The reader asks first; the view asks again right before
                    // it opens the file. Swap after that second answer.
                    if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                        let _ = fs::remove_file(&self.victim);
                        super::symlinks::link(&self.outside, &self.victim);
                    }
                }
                decision
            }
        }
        let victim = at(&fx.root, "a/other");
        let swap = Swap {
            inner: authorizer(&fx.root, ALLOW_WORKSPACE),
            victim: victim.clone(),
            outside: at(&fx.outside, "secret"),
            calls: AtomicUsize::new(0),
        };
        let overlay = Overlay::new(&fx.root);
        let reader = ContainedReader::new(
            BrokerHandle(Arc::new(Broker::default())),
            Seam(overlay),
            swap,
            Some(fx.root.clone()),
            input_limits(&FsReadLimits::DEFAULT),
        );
        match reader.base_fs_read_async(&victim).await {
            Ok(bytes) => panic!("the swapped file was read: {bytes:?}"),
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                assert!(
                    error
                        .to_string()
                        .starts_with("input_view.containment_unsupported")
                );
            }
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.starts_with("input_view.symlink")
                        || text.starts_with("input_view.reparse_point"),
                    "{text}"
                );
                assert!(!text.contains("OUTSIDE"));
            }
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_non_utf8_path_is_refused() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let h = harness();
    let mut bytes = h.fx.root.as_os_str().as_encoded_bytes().to_vec();
    bytes.extend_from_slice(b"/bad-\xff-name");
    let path = PathBuf::from(OsString::from_vec(bytes));
    let error = h.reader.base_fs_read_async(&path).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
    assert!(h.broker.served.lock().unwrap().is_empty());
}

#[tokio::test]
async fn parity_with_the_brokers_alias_resolution() {
    // The copied `resolve_real_path` must agree with what the broker
    // authorizes against, over the same fixtures.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<PathBuf>>>);
    impl CapabilityAuthorizer for Recorder {
        fn authorize(
            &self,
            request: &omni_capabilities::Request<'_>,
        ) -> omni_capabilities::Decision {
            if let omni_capabilities::Request::Fs { path, .. } = request {
                self.0.lock().unwrap().push(path.to_path_buf());
            }
            omni_capabilities::Decision::Allow
        }
    }
    let fx = Fixture::new();
    let recorder = Recorder::default();
    let sys = PolicyEnforcingSys::new(RealSys, recorder.clone());
    let mut cases = vec![
        at(&fx.root, "a/b/file"),
        at(&fx.root, "a/does/not/exist"),
        at(&fx.root, "a/b/file").join("..").join("other"),
    ];
    if omni_test_capabilities::require(Capability::Symlink, &fx.root) {
        let dangling = at(&fx.root, "dangling");
        let target = at(&fx.outside, "nope");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &dangling).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &dangling).unwrap();
        cases.push(dangling);
        let live = at(&fx.root, "live");
        #[cfg(unix)]
        std::os::unix::fs::symlink(at(&fx.root, "a/b"), &live).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(at(&fx.root, "a/b"), &live).unwrap();
        cases.push(live.join("file"));
    }
    for case in cases {
        recorder.0.lock().unwrap().clear();
        let _ = sys.base_fs_read_async(&case).await;
        let seen = recorder.0.lock().unwrap().clone();
        let mine = omni_contained_read::resolve_real_path(&case)
            .unwrap_or_else(|| case.clone());
        assert_eq!(seen, vec![mine], "case {}", case.display());
    }
}

#[tokio::test]
async fn the_plain_seam_resolves_against_the_current_directory() {
    let seam = PlainReadThrough(RealSys);
    let absolute = Fixture::new().root;
    assert_eq!(seam.resolve(&absolute).await.unwrap(), absolute);
    let relative = seam.resolve(Path::new("x/y")).await.unwrap();
    assert!(relative.is_absolute());
    assert!(relative.ends_with(Path::new("x").join("y")));
}
