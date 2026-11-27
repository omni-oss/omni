//! RFC 0024 F06.3 parity: the executor, the dry run and `LoadedContext::
//! get_project_hash` (the `ProjectHasher`) must produce the same task key for
//! the same task. Dependency digests flow through the graph, so tasks with
//! dependencies are covered too.

use std::collections::BTreeMap;

use omni_context::Context;
use omni_execution_plan::TaskSelector;
use omni_hasher::{
    Hasher as _,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::Hash,
};
use omni_messages::NoopSubscriber;
use omni_task_executor::{
    Call, ExecutionConfigBuilder, Force, OnFailure, TaskExecutor,
};
use omni_test_utils::{
    DependencyStrategy, generate_workspace, presets::exec_preset,
};
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;

async fn digests(
    ctx: &omni_context::LoadedContext<RealSys>,
    dry_run: bool,
    ignore_dependencies: bool,
    args: &[(&str, &str)],
) -> BTreeMap<(String, String), DefaultHash> {
    let config = ExecutionConfigBuilder::default()
        .on_failure(OnFailure::Continue)
        .max_concurrency(2usize)
        .force(Force::All)
        .dry_run(dry_run)
        .ignore_dependencies(ignore_dependencies)
        .args(
            args.iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect::<maps::UnorderedMap<_, _>>(),
        )
        .call(Call::exact_tasks(["t2"]))
        .build()
        .expect("config");
    TaskExecutor::new(config, ctx, NoopSubscriber)
        .run()
        .await
        .expect("run")
        .into_iter()
        .filter_map(|r| {
            let hash = r.hash()?;
            let node = r.task();
            Some((
                (
                    node.project_name().to_string(),
                    node.task_name().to_string(),
                ),
                hash,
            ))
        })
        .collect()
}

/// What `hash project` returns for one task: the seed folded with that task's
/// digest.
fn fold(digest: DefaultHash) -> DefaultHash {
    let mut hash =
        Hash::<DefaultHasher>::new(DefaultHasher::hash(b"DEFAULT_SEED"));
    hash.combine_in_place(digest);
    hash.to_inner()
}

#[tokio::test]
async fn the_executor_the_dry_run_and_hash_project_agree_on_every_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = exec_preset(4, 3, DependencyStrategy::Layered);
    generate_workspace(dir.path(), &config).expect("generate");
    let ctx = Context::new(
        RealSys,
        "development",
        dir.path(),
        false,
        "workspace.omni.yaml",
        None,
        &TracingConfig::disabled(),
    )
    .expect("context")
    .into_loaded()
    .await
    .expect("load");

    let real = digests(&ctx, false, false, &[]).await;
    let dry = digests(&ctx, true, false, &[]).await;
    assert!(real.len() > 3, "tasks with dependencies are in the run");
    assert_eq!(real, dry, "dry run and real run");

    for ((project, task), digest) in &real {
        let hashed = ctx
            .get_project_hash(project, &[TaskSelector::exact(task.as_str())])
            .await
            .expect("hash project");
        assert_eq!(hashed, fold(*digest), "hash project for {project}#{task}");
    }

    // An invocation arg is a key input, and `hash project` (which has no
    // invocation) is unaffected by it.
    let with_arg = digests(&ctx, true, false, &[("k", "v")]).await;
    for (key, digest) in &real {
        assert_ne!(
            with_arg.get(key),
            Some(digest),
            "an invocation arg must reach the key of {key:?}"
        );
    }

    // Without dependencies the dependency leaves and the dependency set of the
    // effective definition are gone, so a task with dependencies gets a
    // different key by design.
    let alone = digests(&ctx, true, true, &[]).await;
    assert_eq!(alone.len(), 4, "one `t2` per project");
    for (key, digest) in &alone {
        assert_ne!(real.get(key), Some(digest), "{key:?} has dependencies");
    }
}
