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
    let ctx = load(dir.path()).await;
    assert_parity(&ctx).await;
}

async fn load(dir: &std::path::Path) -> omni_context::LoadedContext<RealSys> {
    Context::new(
        RealSys,
        "development",
        dir,
        false,
        "workspace.omni.yaml",
        None,
        &TracingConfig::disabled(),
    )
    .expect("context")
    .into_loaded()
    .await
    .expect("load")
}

async fn assert_parity(
    ctx: &omni_context::LoadedContext<RealSys>,
) -> BTreeMap<(String, String), DefaultHash> {
    let real = digests(ctx, false, false, &[]).await;
    let dry = digests(ctx, true, false, &[]).await;
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
    let with_arg = digests(ctx, true, false, &[("k", "v")]).await;
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
    let alone = digests(ctx, true, true, &[]).await;
    assert_eq!(alone.len(), 4, "one `t2` per project");
    for (key, digest) in &alone {
        assert_ne!(real.get(key), Some(digest), "{key:?} has dependencies");
    }
    real
}

/// Adds `retry_exec` to every `t2` of a generated workspace.
fn add_retry_exec(dir: &std::path::Path) {
    for entry in std::fs::read_dir(dir.join("packages")).expect("packages") {
        let file = entry.expect("entry").path().join("project.omni.yaml");
        let body = std::fs::read_to_string(&file).expect("read");
        let mut out = String::new();
        for line in body.lines() {
            out.push_str(line);
            out.push('\n');
            if line.trim_start().starts_with("exec: ") && line.ends_with(" t2")
            {
                out.push_str(&line.replacen("exec: ", "retry_exec: ", 1));
                out.push('\n');
            }
        }
        std::fs::write(file, out).expect("write");
    }
}

/// #189 made graph nodes carry `retry_exec`. The effective-task digest, the
/// executor key, the dry-run key, `hash project` and so the workspace hasher
/// must all see it, and it must change the key.
#[tokio::test]
async fn a_task_with_retry_exec_has_the_same_key_everywhere() {
    let plain = tempfile::tempdir().expect("tempdir");
    let retry = tempfile::tempdir().expect("tempdir");
    let config = exec_preset(4, 3, DependencyStrategy::Layered);
    generate_workspace(plain.path(), &config).expect("generate");
    generate_workspace(retry.path(), &config).expect("generate");
    add_retry_exec(retry.path());
    let body = std::fs::read_to_string(
        retry
            .path()
            .join("packages")
            .join("p-0")
            .join("project.omni.yaml"),
    )
    .expect("read");
    assert!(
        body.contains("retry_exec: "),
        "retry_exec was added: {body}"
    );

    let before = assert_parity(&load(plain.path()).await).await;
    let after = assert_parity(&load(retry.path()).await).await;
    assert_eq!(before.len(), after.len());
    for (key, digest) in &before {
        let changed = after.get(key) != Some(digest);
        if key.1 == "t2" {
            assert!(changed, "retry_exec must reach the key of {key:?}");
        }
    }
}
