use std::{borrow::Cow, path::PathBuf};

use bridge_rpc_services::{FsSys, ProcSys};
use maps::UnorderedMap;
use omni_capabilities::{PathRoots, Root};
use omni_configurations::{SourceConfig, Subsystem, types::SingleOrMany};
use omni_context::{Context, ContextSys, LoadedContext};
use omni_input_schema::{ValidationConfig, to_json_schema, validate};
use omni_remote_source::{RemoteSource, RemoteSourceRef, sys::RemoteSourceSys};
use omni_tool::{LazyToolRunner, ToolEnforcement, ToolSys, run_named};
use omni_tool_configurations::ToolConfiguration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{BaseFsMetadataAsync, EnvVars};
use tokio::task::JoinSet;
use value_bag::{OwnedValueBag, ValueBag};

/// Where a tool operates: the base its relative `ctx.sys` paths resolve
/// against. Mutually-exclusive at the CLI (`--cwd` vs `--project`); MCP exposes
/// only the path form.
pub enum ToolWorkingDir {
    /// An explicit directory path, absolute or relative to the current dir.
    Path(PathBuf),
    /// The directory of the named workspace project.
    Project(String),
}

/// Summary of a single tool, returned by `tool_list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
}

/// Response of `tool_list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolListResponse {
    pub tools: Vec<ToolInfo>,
}

/// Response of `tool_inspect`: a tool's identity plus the JSON Schema derived
/// from its own `inputs` block.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolInspectResponse {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// List every discovered tool in the workspace.
pub async fn handle_tool_list<TSys>(
    ctx: &Context<TSys>,
) -> eyre::Result<ToolListResponse>
where
    TSys:
        ContextSys + ToolSys + omni_remote_source::sys::RemoteSourceSys + Clone,
{
    let sys = ctx.sys().clone();
    let tools = get_tools(ctx, &sys).await?;

    let tools = tools
        .iter()
        .map(|t| ToolInfo {
            name: t.name.clone(),
            description: t.description.clone(),
        })
        .collect();

    Ok(ToolListResponse { tools })
}

/// Inspect a single tool, returning the JSON Schema of its own inputs.
pub async fn handle_tool_inspect<TSys>(
    ctx: &Context<TSys>,
    name: &str,
) -> eyre::Result<ToolInspectResponse>
where
    TSys:
        ContextSys + ToolSys + omni_remote_source::sys::RemoteSourceSys + Clone,
{
    let sys = ctx.sys().clone();
    let tools = get_tools(ctx, &sys).await?;

    let tool = tools
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| eyre::eyre!("tool '{name}' not found"))?;

    Ok(ToolInspectResponse {
        name: tool.name.clone(),
        description: tool.description.clone(),
        input_schema: to_json_schema(&tool.inputs),
    })
}

/// Run the tool named `name` with `args`, returning its captured JSON value.
///
/// The `args` object is validated against the tool's declared inputs before
/// execution; validation is identical whether the caller is the CLI or MCP.
/// `working_dir` selects the base the tool's relative `ctx.sys` paths resolve
/// against; when `None` the workspace root is used.
/// Notices are sent through `subscriber` with selected input values redacted;
/// the successful JSON value is returned unchanged. Discovery, validation,
/// working directory resolution, and execution can return errors. Validation
/// and execution errors are redacted using the supplied input values.
pub async fn handle_tool_run<TSys, S>(
    ctx: &LoadedContext<TSys>,
    name: &str,
    args: serde_json::Value,
    working_dir: Option<ToolWorkingDir>,
    redact: &omni_configurations::RedactOverrides,
    subscriber: &S,
) -> eyre::Result<serde_json::Value>
where
    S: omni_messages::NoticeSubscriber,
    TSys: ContextSys
        + ToolSys
        + omni_remote_source::sys::RemoteSourceSys
        + FsSys
        + ProcSys
        + EnvVars
        + Clone,
    <TSys as BaseFsMetadataAsync>::Metadata: Send,
{
    let sys = ctx.sys().clone();
    let tools = get_tools(ctx.as_context(), &sys).await?;

    let tool = tools
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| eyre::eyre!("tool '{name}' not found"))?;

    // Validate the supplied arguments against the tool's declared inputs.
    let values: UnorderedMap<String, OwnedValueBag> = args
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), ValueBag::from_serde1(v).to_owned()))
                .collect()
        })
        .unwrap_or_default();

    let policy =
        redact.apply(omni_configurations::RedactPolicy::resolve(&[&ctx
            .workspace_configuration()
            .redact]));
    // The tool call is its own operation: its notices and failures are
    // sanitized with these inputs' secrets, and the shared subscriber never
    // holds them (RFC 0026).
    let redactor = tool_input_redactor(&tool.inputs, &values, &policy);
    let redacting =
        omni_messages::RedactingSubscriber::new(subscriber, redactor.clone());

    let report = validate(
        &tool.inputs,
        &values,
        &Default::default(),
        &ValidationConfig {
            use_defaults: true,
            redact: Some(omni_input_schema::NameFilter::new(
                policy.name_predicate(
                    omni_configurations::RedactCategory::Inputs,
                ),
            )),
            ..Default::default()
        },
    )
    // A validator can echo a supplied value in its own error (for example a
    // path rule), so this failure is redacted like every other one leaving
    // the tool's scope.
    .map_err(|err| {
        omni_diagnostic::redact_report(err.into(), &|s| {
            redactor.redact(s).into_owned()
        })
    })?;

    if !report.is_valid() {
        let details = report
            .errors
            .iter()
            .map(|e| format!("  - {}: {}", e.input_name, e.message))
            .collect::<Vec<_>>()
            .join("\n");
        let details = redactor.redact(details.as_str());
        return Err(eyre::eyre!(
            "invalid arguments for tool '{name}':\n{details}"
        ));
    }

    let workspace_dir = ctx.root_dir().to_path_buf();

    // Resolve the working directory the tool operates in.
    let resolved_working_dir = match working_dir {
        None => workspace_dir.clone(),
        Some(ToolWorkingDir::Path(path)) => {
            let current_dir = ctx.current_dir()?;
            path_clean::clean(current_dir.join(path))
        }
        Some(ToolWorkingDir::Project(project)) => ctx
            .projects()
            .iter()
            .find(|p| p.name == project)
            .map(|p| p.dir.clone())
            .ok_or_else(|| eyre::eyre!("project '{project}' not found"))?,
    };

    if !sys.fs_is_dir_no_err_async(&resolved_working_dir).await {
        return Err(eyre::eyre!(
            "working directory does not exist or is not a directory: {}",
            resolved_working_dir.display()
        ));
    }

    let runner = LazyToolRunner::new(
        sys,
        workspace_dir.clone(),
        resolved_working_dir,
        env!("CARGO_PKG_VERSION").to_string(),
    );

    // Workspace-level capability floor for the tool subsystem: filter the
    // single subsystem-tagged workspace list down to the entries that govern
    // tools (tag includes `tools`, or `all`), reinterpreting each into the tool
    // profile with its default (unscoped) selector. Folded ahead of — and
    // unwidenable by — each tool's own policy.
    let workspace_floor = ctx
        .workspace_configuration()
        .capabilities
        .rules
        .clone()
        .reinterpret::<omni_tool_configurations::Tool, _>(|scope| {
            scope
                .subsystem
                .includes(Subsystem::Tools)
                .then(omni_capabilities::NoExtra::default)
        });

    // Capability enforcement is an experimental feature; tools run confined
    // only when the workspace has opted in.
    let enforcement = ToolEnforcement {
        workspace_floor,
        roots: PathRoots::new().with(Root::Workspace, workspace_dir),
        workspace_strictness: ctx
            .workspace_configuration()
            .capabilities
            .strictness,
        enforce: ctx
            .workspace_configuration()
            .enable_experimental
            .capabilities(),
    };

    // Notices carry no tool input values, but the operation sanitizes them
    // with the same secrets as errors so a declared secret can never reach a
    // presenter or the trace mirror. They are forwarded while the tool runs,
    // so a warning such as the sandbox downgrade is visible before a
    // long-running tool finishes.
    let mut rx = runner.stream_notices();
    let result = {
        let mut run = std::pin::pin!(run_named(
            &tools,
            name,
            args,
            &runner,
            &enforcement
        ));
        loop {
            tokio::select! {
                biased;
                Some(n) = rx.recv() => {
                    publish_tool_notice(&redacting, n);
                }
                r = &mut run => break r,
            }
        }
    };
    runner.shutdown().await;
    while let Ok(n) = rx.try_recv() {
        publish_tool_notice(&redacting, n);
    }

    match result {
        Ok(value) => Ok(value),
        // Redacted while the tool's inputs are still known: the whole cause
        // chain is sanitized, and a diagnostic keeps its code, details and
        // hints.
        Err(err) => Err(omni_diagnostic::redact_report(err.into(), &|s| {
            redactor.redact(s).into_owned()
        })),
    }
}

/// Delivers a tool's notice through its operation, if the sink wants notices.
fn publish_tool_notice<S: omni_messages::NoticeSubscriber>(
    redacting: &omni_messages::RedactingSubscriber<'_, S>,
    notice: omni_messages::NoticeEvent,
) {
    if omni_messages::NoticeSubscriber::wants_notices(redacting) {
        omni_messages::publish::publish_event(redacting, notice);
    }
}

/// Values of inputs marked `secret` plus those selected by the `redact.inputs`
/// policy, so they never reach error output.
fn tool_input_redactor(
    inputs: &[omni_input_schema::InputSchema],
    values: &UnorderedMap<String, OwnedValueBag>,
    policy: &omni_configurations::RedactPolicy,
) -> omni_redact::Redactor {
    let entries: Vec<(&str, String)> = values
        .iter()
        .map(|(k, v)| (k.as_str(), v.to_string()))
        .collect();
    let secrets = inputs.iter().filter(|i| i.base().secret).filter_map(|i| {
        entries
            .iter()
            .find(|(k, _)| *k == i.base().name.as_str())
            .map(|(_, v)| v.clone())
    });
    let own = omni_redact::Redactor::builder().values(secrets).build();
    own.union(&policy.redactor(
        omni_configurations::RedactCategory::Inputs,
        entries.iter().map(|(k, v)| (*k, v.as_str())),
    ))
}

type RetrievalResult = eyre::Result<(
    Vec<Cow<'static, ToolConfiguration>>,
    Option<RemoteSourceRef>,
)>;

/// Discover and load every tool declared in the workspace's `tools:` sources.
pub async fn get_tools<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<Vec<Cow<'static, ToolConfiguration>>>
where
    TSys: ContextSys + ToolSys + RemoteSourceSys + Clone,
{
    let remote_sources = std::sync::Arc::new(
        crate::operations::remote_source::open_source_store(ctx, sys).await?,
    );

    let mut retrieval_tasks: JoinSet<RetrievalResult> = JoinSet::new();

    for config in ctx.workspace_configuration().tools.iter() {
        match config {
            SourceConfig::Local(local) => {
                let local = local.clone();
                let root_dir = ctx.root_dir().to_path_buf();
                let sys = sys.clone();
                retrieval_tasks.spawn(async move {
                    let configurations = match local.path {
                        SingleOrMany::Single(item) => {
                            omni_tool::discover(&root_dir, &[item], &sys)
                                .await?
                        }
                        SingleOrMany::Many(items) => {
                            omni_tool::discover(&root_dir, &items, &sys).await?
                        }
                    };
                    Ok((configurations, None))
                });
            }
            SourceConfig::Git(git) => {
                let remote_sources = remote_sources.clone();
                let sys = sys.clone();
                let git = git.clone();
                retrieval_tasks.spawn(async move {
                    let source = RemoteSource::Git {
                        uri: git.uri.clone(),
                        rev: git.rev.clone(),
                    };
                    let materialized =
                        remote_sources.materialize(&source).await?;
                    let configurations = omni_tool::discover_in_source(
                        &materialized.root,
                        &["**"],
                        &sys,
                    )
                    .await?;
                    let git_ref = RemoteSourceRef {
                        source,
                        pin: materialized.pin,
                    };
                    Ok((configurations, Some(git_ref)))
                });
            }
            SourceConfig::Registry(_) => {
                unreachable!("registry sources are never constructed")
            }
        }
    }

    let mut configurations = vec![];
    let mut refs = vec![];
    for result in retrieval_tasks.join_all().await {
        let (configs, git_ref) = result?;
        configurations.extend(configs);
        if let Some(git_ref) = git_ref {
            refs.push(git_ref);
        }
    }

    let packs = ctx.workspace_configuration().packs.clone();
    if !packs.is_empty() {
        let (expanded, pack_refs) =
            omni_remote_source_contributors::expand_packs(
                remote_sources.as_ref(),
                &packs,
                ctx.root_dir(),
                false,
                None,
            )
            .await?;
        refs.extend(pack_refs);

        for contributed in expanded.effective_tool_sources() {
            let discovered = match &contributed.source {
                SourceConfig::Local(local) => {
                    let paths: Vec<String> = match &local.path {
                        SingleOrMany::Single(p) => vec![p.clone()],
                        SingleOrMany::Many(ps) => ps.clone(),
                    };
                    if contributed.in_store {
                        omni_tool::discover_in_source(
                            &contributed.root,
                            &paths,
                            sys,
                        )
                        .await?
                    } else {
                        omni_tool::discover(&contributed.root, &paths, sys)
                            .await?
                    }
                }
                SourceConfig::Git(git) => {
                    let source = RemoteSource::Git {
                        uri: git.uri.clone(),
                        rev: git.rev.clone(),
                    };
                    let materialized =
                        remote_sources.materialize(&source).await?;
                    let discovered = omni_tool::discover_in_source(
                        &materialized.root,
                        &["**"],
                        sys,
                    )
                    .await?;
                    refs.push(RemoteSourceRef {
                        source,
                        pin: materialized.pin,
                    });
                    discovered
                }
                SourceConfig::Registry(_) => {
                    unreachable!("registry sources are never constructed")
                }
            };

            configurations
                .extend(namespace_tools(&contributed.qualified_id, discovered));
        }
    }

    remote_sources.record_refs("tool", &refs).await?;
    remote_sources.persist_pins().await?;

    Ok(configurations)
}

/// Namespace every discovered tool's unique name by the pack's qualified id
/// (`<qualified-id>:<name>`), so pack-contributed tools cannot collide with the
/// workspace's own names or another pack's.
fn namespace_tools(
    qualified_id: &str,
    configs: Vec<Cow<'static, ToolConfiguration>>,
) -> Vec<Cow<'static, ToolConfiguration>> {
    configs
        .into_iter()
        .map(|c| {
            let mut c = c.into_owned();
            c.name = format!("{qualified_id}:{}", c.name);
            Cow::Owned(c)
        })
        .collect()
}
