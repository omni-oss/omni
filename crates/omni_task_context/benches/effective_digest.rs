//! Cost of one task's effective-task digest (RFC 0024 F06.3): building the
//! config from a node, the unexpanded cache information and the meta, then
//! encoding and hashing it. It runs once per task per run, in
//! `DefaultTaskContextProvider::get_task_context`.

use std::{hint::black_box, path::PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_task_context::{CacheInfo, effective_task::build};
use omni_types::{OmniPath, TaskKey};

struct Fixture {
    node: TaskExecutionNode,
    info: CacheInfo,
    meta: MetaConfiguration,
}

fn fixture(deps: usize, args: usize, env: usize, globs: usize) -> Fixture {
    let node = TaskExecutionNode::new(
        TaskKey::new("project-name", "build"),
        Some(CommandConfig::Shell(
            "cargo build --release -p {{ project.name }}".to_string(),
        )),
        None,
        PathBuf::from("/ws/project-name"),
        (0..deps)
            .map(|i| TaskKey::new(format!("dep-{i}"), "build"))
            .collect(),
        TeraExprBoolean::Boolean(true),
        false,
        false,
        Default::default(),
    )
    .with_siblings(vec![TaskKey::new("project-name", "lint")]);

    let mut info: CacheInfo = serde_json::from_value(serde_json::json!({
        "cache_enabled": true,
        "key_defaults": true,
        "key_env_keys": ["MODE", "CI"],
        "key_input_files": { "include": [], "exclude": [] },
        "cache_output_files": { "include": [], "exclude": [] },
        "cache_logs": true,
        "args": {},
    }))
    .expect("cache info");
    for i in 0..args {
        info.args
            .insert(format!("arg{i}"), serde_json::json!(format!("value{i}")));
    }
    for i in 0..env {
        info.env
            .project
            .insert(format!("VAR_{i}"), format!("value-{i}"));
    }
    for i in 0..globs {
        info.key_input_files
            .include
            .push(OmniPath::new(format!("src/module{i}/**/*.rs")));
        info.cache_output_files
            .include
            .push(OmniPath::new(format!("dist/part{i}/**")));
    }
    info.key_input_files.exclude.push(OmniPath::new("**/*.tmp"));

    let meta: MetaConfiguration = serde_json::from_value(serde_json::json!({
        "flavor": "release",
        "owner": "team",
        "nested": { "a": [1, 2, 3], "b": { "c": "d" } },
    }))
    .expect("meta");

    Fixture { node, info, meta }
}

fn benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("effective_digest");
    for (name, f) in [
        ("typical", fixture(3, 3, 4, 3)),
        ("large", fixture(40, 30, 40, 30)),
    ] {
        g.bench_function(format!("build/{name}"), |b| {
            b.iter(|| black_box(build(&f.node, Some(&f.info), Some(&f.meta))));
        });
        let config = build(&f.node, Some(&f.info), Some(&f.meta));
        g.bench_function(format!("encode_hash/{name}"), |b| {
            b.iter(|| black_box(config.digest()));
        });
        g.bench_function(format!("build_and_digest/{name}"), |b| {
            b.iter(|| {
                black_box(build(&f.node, Some(&f.info), Some(&f.meta)).digest())
            });
        });
    }
    g.finish();
}

criterion_group!(effective_digest, benches);
criterion_main!(effective_digest);
