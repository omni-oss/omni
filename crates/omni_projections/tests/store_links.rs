//! How a projection from the shared remote-source store is materialized
//! (RFC 0027, section 4.4): `auto` prefers independent owned files (reflink),
//! then a link into the store, then a copy; explicit kinds do exactly what they
//! say.
//!
//! Tests that need a reflink-capable filesystem or a second volume are gated
//! with `omni_test_capabilities`: they skip with a `SKIPPED[...]` marker locally
//! and fail on a CI lane that lists the capability in
//! `OMNI_REQUIRE_CAPABILITIES`. The fall-back tests use the in-memory
//! filesystem, which models a volume that cannot reflink, so they run on every
//! lane.

use std::path::{Path, PathBuf};

use omni_projection_configurations::{ExistingPolicy, LinkKind};
use omni_projections::{
    ApplyOptions, ApplyOutcome, LinkPair, PriorLink, ResolvedKind, apply_link,
};
use omni_test_capabilities::{Capability, gate, require, scratch_dir};
use system_traits::{
    FsCreateDirAllAsync, FsReadAsync, FsReadLinkAsync, FsWriteAsync,
    impls::{InMemorySys, RealSys},
};

fn opts(link: LinkKind, from_store: bool) -> ApplyOptions {
    ApplyOptions {
        link,
        on_existing: ExistingPolicy::Error,
        from_store,
    }
}

async fn apply(
    sys: &RealSys,
    src: &Path,
    dest: &Path,
    link: LinkKind,
    from_store: bool,
) -> omni_projections::Result<ResolvedKind> {
    let pair = LinkPair {
        source_abs: src.to_path_buf(),
        dest_abs: dest.to_path_buf(),
    };
    match apply_link(sys, &pair, &opts(link, from_store), None).await? {
        ApplyOutcome::Linked { kind, .. } => Ok(kind),
        ApplyOutcome::Skipped => panic!("unexpected skip"),
    }
}

/// `store/tree/{a.txt, sub/b.txt}`
fn make_tree(root: &Path) -> PathBuf {
    let tree = root.join("store/tree");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    std::fs::write(tree.join("a.txt"), b"a").unwrap();
    std::fs::write(tree.join("sub/b.txt"), b"b").unwrap();
    tree
}

/// A directory on a different volume than `dir`, when the host has one.
/// Linux: `/dev/shm` (tmpfs) against a disk-backed `dir`.
fn other_volume(dir: &Path) -> Option<tempfile::TempDir> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let other = tempfile::tempdir_in(shm).ok()?;
        let a = std::fs::metadata(dir).ok()?.dev();
        let b = std::fs::metadata(other.path()).ok()?.dev();
        (a != b).then_some(other)
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

#[tokio::test]
async fn auto_from_store_reflinks_files_and_directories_when_supported() {
    let scratch = scratch_dir(Capability::Reflink);
    if !require(Capability::Reflink, scratch.path()) {
        return;
    }
    let tree = make_tree(scratch.path());

    let file_dest = scratch.path().join("ws/a.txt");
    let kind = apply(
        &RealSys,
        &tree.join("a.txt"),
        &file_dest,
        LinkKind::Auto,
        true,
    )
    .await
    .unwrap();
    assert_eq!(kind, ResolvedKind::Reflink);
    assert!(!file_dest.is_symlink());

    let dir_dest = scratch.path().join("ws/tree");
    let kind = apply(&RealSys, &tree, &dir_dest, LinkKind::Auto, true)
        .await
        .unwrap();
    assert_eq!(kind, ResolvedKind::Reflink);
    assert!(dir_dest.is_dir() && !dir_dest.is_symlink());
    assert_eq!(std::fs::read(dir_dest.join("sub/b.txt")).unwrap(), b"b");

    // Independent of the store: editing leaves the tree alone, and deleting the
    // store leaves the projection intact.
    std::fs::write(dir_dest.join("a.txt"), b"edited").unwrap();
    assert_eq!(std::fs::read(tree.join("a.txt")).unwrap(), b"a");
    std::fs::remove_dir_all(scratch.path().join("store")).unwrap();
    assert_eq!(std::fs::read(dir_dest.join("sub/b.txt")).unwrap(), b"b");
    assert_eq!(std::fs::read(&file_dest).unwrap(), b"a");
}

#[tokio::test]
async fn auto_from_store_without_reflink_symlinks_files_by_absolute_path_and_copies_directories()
 {
    // The in-memory filesystem cannot reflink, so this runs on every lane.
    let sys = InMemorySys::default();
    sys.fs_create_dir_all_async("/store/tree").await.unwrap();
    sys.fs_write_async("/store/tree/a.txt", b"a").await.unwrap();
    sys.fs_create_dir_all_async("/ws").await.unwrap();

    // A file: an absolute link into the store, never a hard link.
    let file = LinkPair {
        source_abs: "/store/tree/a.txt".into(),
        dest_abs: "/ws/a.txt".into(),
    };
    let outcome = apply_link(&sys, &file, &opts(LinkKind::Auto, true), None)
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ApplyOutcome::Linked {
            kind: ResolvedKind::Symlink,
            backup: None
        }
    );
    assert_eq!(
        sys.fs_read_link_async("/ws/a.txt").await.unwrap(),
        Path::new("/store/tree/a.txt"),
        "store links are absolute"
    );

    // A directory: copied, because a directory symlink could change the shared
    // tree.
    let dir = LinkPair {
        source_abs: "/store/tree".into(),
        dest_abs: "/ws/tree".into(),
    };
    let outcome = apply_link(&sys, &dir, &opts(LinkKind::Auto, true), None)
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ApplyOutcome::Linked {
            kind: ResolvedKind::Copy,
            backup: None
        }
    );
    assert_eq!(
        sys.fs_read_async("/ws/tree/a.txt").await.unwrap().as_ref(),
        b"a"
    );
}

#[tokio::test]
async fn auto_from_store_surfaces_real_errors_instead_of_falling_through() {
    let dir = tempfile::tempdir().unwrap();
    // A missing source is not "reflink unsupported" (the clone call reports
    // it as an invalid argument): it must be an error, not a symlink to
    // nothing.
    let result = apply(
        &RealSys,
        &dir.path().join("store/missing.txt"),
        &dir.path().join("ws/missing.txt"),
        LinkKind::Auto,
        true,
    )
    .await;
    assert!(result.is_err(), "{result:?}");
    assert!(!dir.path().join("ws/missing.txt").exists());
}

#[tokio::test]
async fn auto_from_store_falls_through_across_volumes_with_a_cause() {
    let scratch = scratch_dir(Capability::Hardlink);
    let Some(other) = other_volume(scratch.path()) else {
        let detail = format!(
            "os={} dir={}: no second volume (needs /dev/shm on a different device)",
            std::env::consts::OS,
            scratch.path().display()
        );
        gate("cross-volume", false, detail);
        return;
    };
    let tree = make_tree(scratch.path());

    // Reflink and hardlink both fail across volumes (CrossesDevices); `auto`
    // must move on to the absolute symlink rather than erroring.
    let file = tree.join("a.txt");
    let linked = other.path().join("a.txt");
    let kind = apply(&RealSys, &file, &linked, LinkKind::Auto, true)
        .await
        .unwrap();
    assert_eq!(kind, ResolvedKind::Symlink);
    assert_eq!(RealSys.fs_read_link_async(&linked).await.unwrap(), file);

    // A directory is copied across volumes rather than linked.
    let copied = other.path().join("tree");
    let kind = apply(&RealSys, &tree, &copied, LinkKind::Auto, true)
        .await
        .unwrap();
    assert_eq!(kind, ResolvedKind::Copy);
    assert_eq!(std::fs::read(copied.join("sub/b.txt")).unwrap(), b"b");
}

#[tokio::test]
async fn explicit_hardlink_links_a_directory_file_by_file() {
    let scratch = scratch_dir(Capability::Hardlink);
    if !require(Capability::Hardlink, scratch.path()) {
        return;
    }
    let tree = make_tree(scratch.path());
    let dest = scratch.path().join("ws/tree");

    let kind = apply(&RealSys, &tree, &dest, LinkKind::Hardlink, true)
        .await
        .unwrap();
    assert_eq!(kind, ResolvedKind::Hardlink);
    assert!(dest.is_dir() && !dest.is_symlink());
    assert_eq!(std::fs::read(dest.join("sub/b.txt")).unwrap(), b"b");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(dest.join("a.txt")).unwrap().ino(),
            std::fs::metadata(tree.join("a.txt")).unwrap().ino()
        );
    }
}

#[tokio::test]
async fn explicit_hardlink_is_strict_and_leaves_no_partial_tree() {
    let scratch = scratch_dir(Capability::Hardlink);
    let Some(other) = other_volume(scratch.path()) else {
        let detail = format!(
            "os={} dir={}: no second volume",
            std::env::consts::OS,
            scratch.path().display()
        );
        gate("cross-volume", false, detail);
        return;
    };
    let tree = make_tree(scratch.path());
    let dest = other.path().join("tree");

    let result = apply(&RealSys, &tree, &dest, LinkKind::Hardlink, true).await;
    assert!(result.is_err(), "hardlink across volumes must error");
    assert!(!dest.exists(), "no partial tree may remain");
}

#[tokio::test]
async fn explicit_symlink_from_store_is_absolute_and_dangles_when_the_store_goes()
 {
    let scratch = scratch_dir(Capability::Symlink);
    if !require(Capability::Symlink, scratch.path()) {
        return;
    }
    let tree = make_tree(scratch.path());
    let dest = scratch.path().join("ws/tree");

    let kind = apply(&RealSys, &tree, &dest, LinkKind::Symlink, true)
        .await
        .unwrap();
    assert_eq!(kind, ResolvedKind::Symlink);
    assert_eq!(RealSys.fs_read_link_async(&dest).await.unwrap(), tree);

    std::fs::remove_dir_all(scratch.path().join("store")).unwrap();
    assert!(!dest.exists(), "the link dangles once the store is deleted");
}

#[tokio::test]
async fn local_sources_keep_relative_symlinks() {
    let scratch = scratch_dir(Capability::Symlink);
    if !require(Capability::Symlink, scratch.path()) {
        return;
    }
    let tree = make_tree(scratch.path());
    let dest = scratch.path().join("ws/a.txt");

    let kind =
        apply(&RealSys, &tree.join("a.txt"), &dest, LinkKind::Auto, false)
            .await
            .unwrap();
    assert_eq!(kind, ResolvedKind::Symlink);
    assert!(
        RealSys
            .fs_read_link_async(&dest)
            .await
            .unwrap()
            .is_relative()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_unchanged_pin_repoints_a_link_into_a_moved_store_and_repairs_a_dangling_one()
 {
    let scratch = scratch_dir(Capability::Symlink);
    if !require(Capability::Symlink, scratch.path()) {
        return;
    }
    let old_tree = make_tree(&scratch.path().join("old"));
    let new_tree = make_tree(&scratch.path().join("new"));
    let dest = scratch.path().join("ws/tree");
    let options = opts(LinkKind::Symlink, true);
    let old_pair = LinkPair {
        source_abs: old_tree,
        dest_abs: dest.clone(),
    };
    apply_link(&RealSys, &old_pair, &options, None)
        .await
        .unwrap();

    // The engine maps a moved store's old link to the new source path.
    let pair = LinkPair {
        source_abs: new_tree.clone(),
        dest_abs: dest.clone(),
    };
    let prior = PriorLink {
        source_abs: &new_tree,
        pin_unchanged: true,
    };

    // The old store still resolves but the link points into it: re-point it now
    // instead of leaving it to dangle once the old store is deleted.
    let outcome = apply_link(&RealSys, &pair, &options, Some(&prior))
        .await
        .unwrap();
    assert!(
        matches!(outcome, ApplyOutcome::Linked { .. }),
        "{outcome:?}"
    );
    assert_eq!(RealSys.fs_read_link_async(&dest).await.unwrap(), new_tree);

    // A link that already points at the current store is left alone.
    let outcome = apply_link(&RealSys, &pair, &options, Some(&prior))
        .await
        .unwrap();
    assert_eq!(outcome, ApplyOutcome::Skipped);

    // A link whose store vanished is repaired.
    std::fs::remove_file(&dest).unwrap();
    std::os::unix::fs::symlink(scratch.path().join("gone/tree"), &dest)
        .unwrap();
    assert!(!dest.exists());
    let outcome = apply_link(&RealSys, &pair, &options, Some(&prior))
        .await
        .unwrap();
    assert!(
        matches!(outcome, ApplyOutcome::Linked { .. }),
        "{outcome:?}"
    );
    assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"a");
}

#[tokio::test]
async fn copies_and_reflinks_of_sealed_store_files_are_writable_but_hardlinks_stay_shared()
 {
    let scratch = scratch_dir(Capability::Hardlink);
    if !require(Capability::Hardlink, scratch.path()) {
        return;
    }
    let tree = make_tree(scratch.path());
    // Seal the tree like the store does: files read-only.
    for file in [tree.join("a.txt"), tree.join("sub/b.txt")] {
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).unwrap();
    }

    let copied = scratch.path().join("ws/copy");
    apply(&RealSys, &tree, &copied, LinkKind::Copy, true)
        .await
        .unwrap();
    std::fs::write(copied.join("a.txt"), b"mine").expect("a copy is writable");
    assert_eq!(std::fs::read(tree.join("a.txt")).unwrap(), b"a");

    // A hard link shares the sealed inode, so an in-place edit is refused
    // instead of silently changing what every workspace reads.
    let linked = scratch.path().join("ws/linked");
    apply(&RealSys, &tree, &linked, LinkKind::Hardlink, true)
        .await
        .unwrap();
    assert!(std::fs::write(linked.join("a.txt"), b"nope").is_err());
    assert_eq!(std::fs::read(tree.join("a.txt")).unwrap(), b"a");
}

#[cfg(unix)]
#[tokio::test]
async fn preflight_treats_a_moved_stores_old_link_as_owned_not_a_conflict() {
    use omni_projections::{
        ExistingConflict, Ledger, LedgerLink, PlannedItem,
        existing_file_conflicts,
    };

    let scratch = tempfile::tempdir().unwrap();
    let old_root = scratch.path().join("old/trees/slug/abc");
    let new_root = scratch.path().join("new/trees/slug/abc");
    std::fs::create_dir_all(&old_root).unwrap();
    std::fs::create_dir_all(&new_root).unwrap();
    std::fs::write(new_root.join("a.txt"), b"a").unwrap();
    let dest = scratch.path().join("ws/a.txt");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    // The link omni made into the old store, which is now gone.
    std::os::unix::fs::symlink(old_root.join("a.txt"), &dest).unwrap();

    let ledger = Ledger::from_links(vec![LedgerLink {
        source_id: "src".into(),
        dest: "a.txt".into(),
        target: old_root.join("a.txt").to_string_lossy().into_owned(),
        kind: ResolvedKind::Symlink,
        source_pin: "abc".into(),
        backup: None,
    }]);
    let item = |source_root: &Path| PlannedItem {
        source_id: "src".into(),
        pair: LinkPair {
            source_abs: source_root.join("a.txt"),
            dest_abs: dest.clone(),
        },
        dest_rel: "a.txt".into(),
        is_dir_link: false,
        on_existing: ExistingPolicy::Error,
        link: LinkKind::Auto,
        pin: "abc".into(),
        from_store: true,
        source_root: source_root.to_path_buf(),
    };

    let conflicts: Vec<ExistingConflict> =
        existing_file_conflicts(&RealSys, &[item(&new_root)], &ledger)
            .await
            .unwrap();
    assert!(conflicts.is_empty(), "{conflicts:?}");

    // A different file at the same destination is still foreign.
    let other = PlannedItem {
        pair: LinkPair {
            source_abs: new_root.join("b.txt"),
            dest_abs: dest.clone(),
        },
        ..item(&new_root)
    };
    let conflicts = existing_file_conflicts(&RealSys, &[other], &ledger)
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_inside_a_source_tree_are_reproduced_not_followed() {
    let scratch = scratch_dir(Capability::Hardlink);
    if !require(Capability::Hardlink, scratch.path()) {
        return;
    }
    let secret = scratch.path().join("private.txt");
    std::fs::write(&secret, b"outside the source").unwrap();
    let tree = make_tree(scratch.path());
    std::os::unix::fs::symlink(&secret, tree.join("escape")).unwrap();
    std::os::unix::fs::symlink(scratch.path(), tree.join("sub/up")).unwrap();

    for (kind, name) in [(LinkKind::Copy, "copy"), (LinkKind::Hardlink, "hard")]
    {
        let dest = scratch.path().join(format!("ws/{name}"));
        apply(&RealSys, &tree, &dest, kind, true).await.unwrap();
        for link in ["escape", "sub/up"] {
            let meta = std::fs::symlink_metadata(dest.join(link)).unwrap();
            assert!(meta.is_symlink(), "{name}: {link} must stay a link");
        }
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"a");
    }
}
