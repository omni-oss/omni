use std::{borrow::Cow, path::PathBuf};

use bridge_rpc_services::{FsSys, ProcSys};
use maps::UnorderedMap;
use omni_capabilities::{PathRoots, Root};
use omni_configurations::{SourceConfig, Subsystem, types::SingleOrMany};
use omni_context::{Context, ContextSys, LoadedContext};
use omni_input_schema::{ValidationConfig, to_json_schema, validate};
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
    TSys: ContextSys + ToolSys + Clone,
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
    TSys: ContextSys + ToolSys + Clone,
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
pub async fn handle_tool_run<TSys>(
    ctx: &LoadedContext<TSys>,
    name: &str,
    args: serde_json::Value,
    working_dir: Option<ToolWorkingDir>,
) -> eyre::Result<serde_json::Value>
where
    TSys: ContextSys + ToolSys + FsSys + ProcSys + EnvVars + Clone,
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

    let report = validate(
        &tool.inputs,
        &values,
        &Default::default(),
        &ValidationConfig {
            use_defaults: true,
            ..Default::default()
        },
    )?;

    if !report.is_valid() {
        let details = report
            .errors
            .iter()
            .map(|e| format!("  - {}: {}", e.input_name, e.message))
            .collect::<Vec<_>>()
            .join("\n");
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

    let result = run_named(&tools, name, args, &runner, &enforcement).await;
    runner.shutdown().await;

    Ok(result?)
}

/// Discover and load every tool declared in the workspace's `tools:` sources.
///
/// v1 resolves `local` sources only; `git` sources are reserved for a later
/// revision and are currently ignored.
pub async fn get_tools<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<Vec<Cow<'static, ToolConfiguration>>>
where
    TSys: ContextSys + ToolSys + Clone,
{
    let mut retrieval_tasks: JoinSet<
        eyre::Result<Vec<Cow<'static, ToolConfiguration>>>,
    > = JoinSet::new();

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
                    Ok(configurations)
                });
            }
            // Remote (`git`) tool sources are reserved for a later revision.
            SourceConfig::Git(_) => {}
        }
    }

    let mut configurations = vec![];
    for configs in retrieval_tasks.join_all().await {
        configurations.extend(configs?);
    }

    Ok(configurations)
}
