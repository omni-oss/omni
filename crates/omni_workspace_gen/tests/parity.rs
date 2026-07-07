//! Cross-language parity fixtures.
//!
//! These golden payloads lock the Rust-native core and the wasm-wrapped core
//! (consumed by `scripts/task-bench`) to the *same* output. This Rust test and
//! the task-bench `parity.spec.ts` both assert against the committed
//! `tests/golden/*.json`, so any divergence in the graph, task edges, naming,
//! or rendered omni files fails loudly instead of silently.
//!
//! To regenerate the fixtures after an intentional change:
//! `UPDATE_GOLDEN=1 cargo test -p omni_workspace_gen --test parity`

use std::{collections::BTreeMap, fs, path::PathBuf};

use omni_workspace_gen::{
    DependencyConfig, DependencyStrategy, HarnessConfig, OmniRenderOptions,
    build_model, render_omni,
};
use serde_json::{Value, json};

/// The fixed omni render options used for parity (must match the task-bench
/// `OMNI_RENDER_OPTIONS`).
fn options() -> OmniRenderOptions {
    OmniRenderOptions {
        task_command_template: "node ./task.mjs {task_id}".to_string(),
        project_cache_key_files: vec![
            "package.json".to_string(),
            "task.mjs".to_string(),
            "src/**/*.js".to_string(),
        ],
    }
}

fn cases() -> Vec<(&'static str, HarnessConfig)> {
    let with = |strategy: DependencyStrategy, projects, tasks, seed, edge| {
        HarnessConfig::builder()
            .projects(projects)
            .tasks_per_project(tasks)
            .seed(seed)
            .dependency(
                DependencyConfig::builder()
                    .strategy(strategy)
                    .edge_probability(edge)
                    .build(),
            )
            .build()
    };

    vec![
        (
            "isolated",
            with(DependencyStrategy::Isolated, 4, 2, 1, 0.35),
        ),
        ("chain", with(DependencyStrategy::Chain, 5, 2, 1, 0.35)),
        ("fan-out", with(DependencyStrategy::FanOut, 4, 3, 1, 0.35)),
        ("layered", with(DependencyStrategy::Layered, 8, 3, 1, 0.35)),
        ("random", with(DependencyStrategy::Random, 10, 2, 1234, 0.4)),
    ]
}

fn payload(config: &HarnessConfig) -> Value {
    let model = build_model(config);
    let omni: BTreeMap<String, String> =
        render_omni(&model, &options()).into_iter().collect();
    json!({ "model": model, "omni": omni })
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(format!("{name}.json"))
}

#[test]
fn model_and_omni_match_goldens() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();

    for (name, config) in cases() {
        let actual = payload(&config);
        let path = golden_path(name);

        if update {
            fs::create_dir_all(path.parent().unwrap()).expect("mkdir golden");
            let pretty =
                serde_json::to_string_pretty(&actual).expect("serialize");
            fs::write(&path, format!("{pretty}\n")).expect("write golden");
            continue;
        }

        let raw = fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; regenerate with UPDATE_GOLDEN=1",
                path.display()
            )
        });
        let expected: Value = serde_json::from_str(&raw).expect("parse golden");
        assert_eq!(actual, expected, "parity golden mismatch for `{name}`");
    }
}
