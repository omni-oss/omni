//! Determinism guarantees for the generation harness.
//!
//! The benchmarks built on top of `omni_test_utils` compare timings across
//! runs, so a given preset must produce a **byte-identical** workspace every
//! time it is generated — even when the dependency graph uses the `random`
//! strategy (which is seeded). These tests lock that guarantee in.

use std::{collections::BTreeMap, fs, path::Path};

use omni_test_utils::{
    DependencyConfig, DependencyStrategy, HarnessConfig, generate_workspace,
};

/// Read an entire directory tree into a map of workspace-relative path ->
/// file contents, so two generated workspaces can be compared exactly.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .expect("read_dir")
            .map(|e| e.expect("dir entry").path())
            .collect();
        entries.sort();

        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("strip prefix")
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = fs::read(&path).expect("read file");
                out.insert(rel, bytes);
            }
        }
    }

    out
}

fn assert_identical(config: &HarnessConfig) {
    let a = tempfile::tempdir().expect("tempdir a");
    let b = tempfile::tempdir().expect("tempdir b");

    let nodes_a = generate_workspace(a.path(), config).expect("generate a");
    let nodes_b = generate_workspace(b.path(), config).expect("generate b");

    assert_eq!(
        nodes_a.len(),
        nodes_b.len(),
        "graphs should have the same number of projects"
    );
    for (x, y) in nodes_a.iter().zip(nodes_b.iter()) {
        assert_eq!(x.name, y.name);
        assert_eq!(
            x.dependencies, y.dependencies,
            "edges must be identical run-to-run"
        );
    }

    let snap_a = snapshot(a.path());
    let snap_b = snapshot(b.path());

    assert_eq!(
        snap_a.keys().collect::<Vec<_>>(),
        snap_b.keys().collect::<Vec<_>>(),
        "the same set of files must be generated each run"
    );
    for (path, bytes_a) in &snap_a {
        assert_eq!(
            bytes_a,
            snap_b.get(path).expect("file present in both"),
            "file {path} must be byte-identical run-to-run"
        );
    }
}

#[test]
fn layered_workspace_is_reproducible() {
    let config = HarnessConfig::builder()
        .projects(40)
        .tasks_per_project(3)
        .dependency(
            DependencyConfig::builder()
                .strategy(DependencyStrategy::Layered)
                .build(),
        )
        .build();
    assert_identical(&config);
}

#[test]
fn random_workspace_is_reproducible_for_a_seed() {
    let config = HarnessConfig::builder()
        .seed(1234)
        .projects(60)
        .tasks_per_project(2)
        .dependency(
            DependencyConfig::builder()
                .strategy(DependencyStrategy::Random)
                .edge_probability(0.4)
                .build(),
        )
        .build();
    assert_identical(&config);
}

#[test]
fn different_seeds_change_the_random_graph() {
    let base = |seed: u32| {
        HarnessConfig::builder()
            .seed(seed)
            .projects(60)
            .dependency(
                DependencyConfig::builder()
                    .strategy(DependencyStrategy::Random)
                    .edge_probability(0.4)
                    .build(),
            )
            .build()
    };

    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let g1 = generate_workspace(dir1.path(), &base(1)).unwrap();
    let g2 = generate_workspace(dir2.path(), &base(2)).unwrap();

    let edges1: Vec<_> = g1.iter().map(|n| &n.dependencies).collect();
    let edges2: Vec<_> = g2.iter().map(|n| &n.dependencies).collect();
    assert_ne!(
        edges1, edges2,
        "distinct seeds should yield distinct random graphs"
    );
}
