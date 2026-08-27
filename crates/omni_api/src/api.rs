use std::path::PathBuf;

use omni_cache::{CacheStats, PrunedCacheEntry};
use omni_configurations::ProjectConfiguration;
use omni_context::{
    Context, ContextSys, LoadedContext, MaybeLoaded, WorkspaceInitConfig,
    get_root_dir,
};
use omni_generator::GeneratorSys;
use omni_messages::{OmniEventSubscriber, TracingSubscriber};
use omni_task_executor::TaskExecutorSys;
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys as RealSysSync;
use tokio::sync::Mutex;

use crate::operations::generator::InspectViewKind;
use crate::{
    OmniApiError,
    operations::{
        cache::{
            CachePruneRequest, CachePruneResponse, CacheRemoteSetupRequest,
            CacheStatsRequest,
        },
        config_schema::{ConfigSchemaResponse, SchemaKind},
        env::{EnvRequest, EnvResponse},
        exec::{ExecRequest, ExecResponse},
        generator::{
            GeneratorInspectResponse, GeneratorListResponse,
            GeneratorRunRequest, GeneratorRunResponse,
            GeneratorValidateInputRequest, GeneratorValidateInputResponse,
        },
        hash::HashResponse,
        task::{TaskRunRequest, TaskRunResponse},
    },
    setup_guard::SetupGuard,
};

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for [`OmniApi`].
///
/// Start with [`OmniApi::builder()`] which uses the zero-cost
/// [`TracingSubscriber`] by default.
///
/// # Tracing
/// `OmniApi` **never** initialises a tracing subscriber; that is always an
/// external concern.  Provide [`TracingConfig::disabled()`] (the default) if
/// you do not want any file-based tracing output.
///
/// # Examples
///
/// ```no_run
/// use omni_api::OmniApi;
/// use omni_messages::NoopSubscriber;
///
/// # #[tokio::main] async fn main() -> eyre::Result<()> {
/// // Build an API instance backed by the workspace in the current directory.
/// let api = OmniApi::builder()
///     .subscriber(NoopSubscriber)
///     .with_setup(false)   // skip keyring in library contexts
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct OmniApiBuilder<S: OmniEventSubscriber = TracingSubscriber> {
    subscriber: S,
    with_setup: bool,
    workspace_config: WorkspaceInitConfig,
    tracing_config: TracingConfig,
    root_dir: Option<PathBuf>,
}

impl<S: OmniEventSubscriber> OmniApiBuilder<S> {
    /// Replace the event subscriber.
    pub fn subscriber<S2: OmniEventSubscriber>(
        self,
        subscriber: S2,
    ) -> OmniApiBuilder<S2> {
        OmniApiBuilder {
            subscriber,
            with_setup: self.with_setup,
            workspace_config: self.workspace_config,
            tracing_config: self.tracing_config,
            root_dir: self.root_dir,
        }
    }

    /// Whether to call `omni_setup::initialize()` during [`build`].
    ///
    /// Defaults to `true`. Set to `false` in memory/test contexts where the
    /// keyring is unavailable.
    ///
    /// [`build`]: OmniApiBuilder::build
    pub fn with_setup(mut self, yes: bool) -> Self {
        self.with_setup = yes;
        self
    }

    /// Override workspace initialisation parameters.
    ///
    /// Defaults to [`WorkspaceInitConfig::default()`].
    pub fn workspace_config(mut self, cfg: WorkspaceInitConfig) -> Self {
        self.workspace_config = cfg;
        self
    }

    /// Override the tracing configuration.
    ///
    /// Defaults to [`TracingConfig::disabled()`].
    pub fn tracing_config(mut self, cfg: TracingConfig) -> Self {
        self.tracing_config = cfg;
        self
    }

    /// Pin the workspace root directory.
    ///
    /// If not set the root is auto-detected by walking ancestor directories
    /// for the workspace marker file.
    pub fn root_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.root_dir = Some(path.into());
        self
    }

    /// Consumes the builder and constructs an `OmniApi<RealSysSync, S>`.
    #[allow(clippy::result_large_err)]
    pub fn build(self) -> Result<OmniApi<RealSysSync, S>, OmniApiError> {
        let sys = RealSysSync;

        let root_dir = match self.root_dir {
            Some(r) => r,
            None => get_root_dir(&sys)?,
        };

        let ctx = Context::new(
            sys,
            &self.workspace_config.env,
            &root_dir,
            self.workspace_config.inherit_env_vars,
            &self.workspace_config.env_root_dir_marker,
            self.workspace_config.env_files,
            &self.tracing_config,
        )?;

        let setup_guard = if self.with_setup {
            omni_setup::initialize(omni_setup::InitConfig::builder().build())
                .map_err(OmniApiError::SetupInit)?;
            Some(SetupGuard)
        } else {
            None
        };

        Ok(OmniApi {
            ctx: Mutex::new(MaybeLoaded::Unloaded(ctx)),
            subscriber: self.subscriber,
            _setup_guard: setup_guard,
        })
    }
}

// ── OmniApi ───────────────────────────────────────────────────────────────────

/// The primary workspace API facade.
///
/// Generic over:
/// - `TSys` — the system trait implementation (defaults to the real filesystem)
/// - `S` — the event subscriber (defaults to [`TracingSubscriber`], a zero-cost
///   pass-through that emits `tracing::*` calls)
///
/// All async methods pass `&self.subscriber` to the underlying engines via the
/// blanket `impl ExecutionEventSubscriber for &S` defined in `omni_messages`.
///
/// # Sharing
///
/// `OmniApi` uses a [`tokio::sync::Mutex`] for interior mutability so that the
/// context can be lazily loaded on the first call that requires it. All methods
/// take `&self`, making it straightforward to share an instance via
/// `Arc<OmniApi<...>>`.
pub struct OmniApi<
    TSys: ContextSys = RealSysSync,
    S: OmniEventSubscriber = TracingSubscriber,
> {
    /// Lazily-loaded context behind a mutex. Transitions from `Unloaded` to
    /// `Loaded` on the first operation that requires project metadata, then
    /// stays loaded for the lifetime of this instance.
    ctx: Mutex<MaybeLoaded<TSys>>,
    subscriber: S,
    _setup_guard: Option<SetupGuard>,
}

// ── Construction ──────────────────────────────────────────────────────────────

impl OmniApi<RealSysSync, TracingSubscriber> {
    /// Create a builder with [`TracingSubscriber`] as the default subscriber.
    pub fn builder() -> OmniApiBuilder<TracingSubscriber> {
        OmniApiBuilder {
            subscriber: TracingSubscriber,
            with_setup: true,
            workspace_config: WorkspaceInitConfig::default(),
            tracing_config: TracingConfig::disabled(),
            root_dir: None,
        }
    }
}

impl<TSys: ContextSys, S: OmniEventSubscriber> OmniApi<TSys, S> {
    /// Construct an `OmniApi` from an already-built [`Context`].
    ///
    /// Useful for memory/test systems where the caller manages the setup
    /// lifecycle (set `with_setup = false` via the builder, or use this
    /// constructor directly).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use omni_api::OmniApi;
    /// use omni_context::{Context, WorkspaceInitConfig};
    /// use omni_messages::NoopSubscriber;
    /// use omni_tracing_subscriber::TracingConfig;
    /// use system_traits::impls::RealSys;
    ///
    /// # fn main() -> eyre::Result<()> {
    /// let ctx = Context::new(
    ///     RealSys::default(),
    ///     "development",
    ///     std::path::Path::new("."),
    ///     false,
    ///     "workspace.omni.yaml",
    ///     None,
    ///     &TracingConfig::disabled(),
    /// )?;
    /// let api = OmniApi::new_with_sys(ctx, NoopSubscriber);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new_with_sys(ctx: Context<TSys>, subscriber: S) -> Self {
        Self {
            ctx: Mutex::new(MaybeLoaded::Unloaded(ctx)),
            subscriber,
            _setup_guard: None,
        }
    }

    /// Construct an `OmniApi` from an already-loaded [`LoadedContext`].
    ///
    /// Use this when the caller has already paid the project-discovery cost
    /// (e.g. a CLI command that loaded the context for prompting). Operations
    /// that would otherwise call `into_loaded` internally will skip that work
    /// entirely.
    pub fn new_with_loaded_sys(
        ctx: LoadedContext<TSys>,
        subscriber: S,
    ) -> Self {
        Self {
            ctx: Mutex::new(MaybeLoaded::Loaded(ctx)),
            subscriber,
            _setup_guard: None,
        }
    }

    /// Returns a reference to the event subscriber.
    pub fn subscriber(&self) -> &S {
        &self.subscriber
    }
}

// ── Task-execution operations ─────────────────────────────────────────────────

impl<TSys, S> OmniApi<TSys, S>
where
    TSys: TaskExecutorSys + Clone,
    S: OmniEventSubscriber,
{
    /// Execute one or more named tasks.
    pub async fn task_run(
        &self,
        req: TaskRunRequest,
    ) -> eyre::Result<TaskRunResponse> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::task::handle_task_run(
            ctx.ensure_loaded().await?,
            &self.subscriber,
            req,
        )
        .await
    }

    /// Execute an arbitrary command in the workspace environment.
    pub async fn exec(&self, req: ExecRequest) -> eyre::Result<ExecResponse> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::exec::handle_exec(
            ctx.ensure_loaded().await?,
            &self.subscriber,
            req,
        )
        .await
    }

    /// Compute the hash for the entire workspace.
    pub async fn hash_workspace(&self) -> eyre::Result<HashResponse> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::hash::handle_hash_workspace(
            ctx.ensure_loaded().await?,
        )
        .await
    }

    /// Compute the hash for a single project.
    ///
    /// If `tasks` is empty all tasks are included; otherwise only the listed
    /// task names are hashed.
    pub async fn hash_project(
        &self,
        name: &str,
        tasks: &[String],
    ) -> eyre::Result<HashResponse> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::hash::handle_hash_project(
            ctx.ensure_loaded().await?,
            name,
            tasks,
        )
        .await
    }

    /// Show per-project cache statistics.
    pub async fn cache_stats(
        &self,
        req: CacheStatsRequest,
    ) -> eyre::Result<CacheStats> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::cache::handle_cache_stats(
            ctx.ensure_loaded().await?,
            req,
        )
        .await
    }

    /// Compute prunable cache entries.
    ///
    /// When `req.dry_run == true` the entries are computed but not deleted.
    /// Pass the returned entries to [`cache_force_prune`] to actually remove
    /// them.
    ///
    /// [`cache_force_prune`]: OmniApi::cache_force_prune
    pub async fn cache_prune(
        &self,
        req: CachePruneRequest,
    ) -> eyre::Result<CachePruneResponse> {
        let mut ctx = self.ctx.lock().await;
        crate::operations::cache::handle_cache_prune(
            ctx.ensure_loaded().await?,
            req,
        )
        .await
    }

    /// Delete the entries returned by a previous [`cache_prune`] call.
    ///
    /// [`cache_prune`]: OmniApi::cache_prune
    pub async fn cache_force_prune(
        &self,
        entries: Vec<PrunedCacheEntry>,
    ) -> eyre::Result<()> {
        let ctx = self.ctx.lock().await;
        crate::operations::cache::handle_cache_force_prune(
            ctx.as_context(),
            entries,
        )
        .await
    }

    /// Configure a remote cache server for this workspace.
    pub async fn cache_remote_setup(
        &self,
        req: CacheRemoteSetupRequest,
    ) -> eyre::Result<()> {
        let ctx = self.ctx.lock().await;
        crate::operations::cache::handle_cache_remote_setup(
            ctx.as_context(),
            req,
        )
        .await
    }
}

// ── Environment / workspace-info operations ───────────────────────────────────

impl<TSys, S> OmniApi<TSys, S>
where
    TSys: ContextSys,
    S: OmniEventSubscriber,
{
    /// Retrieve workspace environment variables.
    pub async fn get_env(&self, req: EnvRequest) -> eyre::Result<EnvResponse> {
        let ctx = self.ctx.lock();
        crate::operations::env::handle_env(ctx.await.as_context(), req)
    }

    /// Return the local cache directory path.
    pub async fn cache_dir(&self) -> PathBuf {
        let ctx = self.ctx.lock().await;
        ctx.as_context().cache_dir()
    }

    /// Return a JSON Schema for the requested configuration kind.
    ///
    /// This is a pure, synchronous operation — no workspace loading required.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use omni_api::{handle_config_schema, SchemaKind};
    ///
    /// let resp = handle_config_schema(SchemaKind::Workspace).expect("schema generation");
    /// assert!(resp.schema.is_object());
    /// ```
    pub fn config_schema(
        &self,
        kind: SchemaKind,
    ) -> eyre::Result<ConfigSchemaResponse> {
        crate::operations::config_schema::handle_config_schema(kind)
    }

    /// List the names of all projects in the workspace.
    pub async fn project_list(&self) -> eyre::Result<Vec<String>> {
        let ctx = self.ctx.lock().await;
        crate::operations::project::handle_project_list(ctx.as_context()).await
    }

    /// Return the full configuration for the named project.
    ///
    /// Returns an error if no project with that name exists.
    pub async fn project_config(
        &self,
        name: &str,
    ) -> eyre::Result<ProjectConfiguration> {
        let ctx = self.ctx.lock().await;
        crate::operations::project::handle_project_config(
            ctx.as_context(),
            name,
        )
        .await
    }
}

// ── Generator operations ──────────────────────────────────────────────────────

impl<TSys, S> OmniApi<TSys, S>
where
    TSys: ContextSys + GeneratorSys + Clone,
    S: OmniEventSubscriber,
{
    /// Run a generator against the workspace.
    ///
    /// The `req.name` field is required; the CLI adapter must prompt the user
    /// for it before building this request.
    pub async fn generator_run(
        &self,
        req: GeneratorRunRequest,
    ) -> eyre::Result<GeneratorRunResponse> {
        let mut ctx = self.ctx.lock().await;
        ctx.load().await?;
        crate::operations::generator::handle_generator_run(
            ctx.as_loaded_context(),
            &self.subscriber,
            req,
        )
        .await
    }

    /// List all available generators in the workspace.
    pub async fn generator_list(&self) -> eyre::Result<GeneratorListResponse> {
        let ctx = self.ctx.lock().await;
        crate::operations::generator::handle_generator_list(ctx.as_context())
            .await
    }

    /// Inspect a generator's full input schema, recursing into all
    /// sub-generators it invokes via `run-generator` actions. Cycle-safe.
    ///
    /// `view` selects the input projection: use `InspectViewKind::Widget`
    /// (default) for interactive UIs and `InspectViewKind::Data` for machine
    /// consumers such as MCP.
    pub async fn generator_inspect(
        &self,
        name: &str,
        view: InspectViewKind,
    ) -> eyre::Result<GeneratorInspectResponse> {
        let ctx = self.ctx.lock().await;
        crate::operations::generator::handle_generator_inspect(
            ctx.as_context(),
            name,
            view,
        )
        .await
    }

    /// Validate a set of input values against a generator's schema.
    ///
    /// Does not run the generator. Returns a list of validation errors (missing
    /// required fields, failed validator expressions) so that callers can give
    /// feedback before attempting a full run.
    pub async fn generator_validate_input(
        &self,
        req: GeneratorValidateInputRequest,
    ) -> eyre::Result<GeneratorValidateInputResponse> {
        let ctx = self.ctx.lock().await;
        crate::operations::generator::handle_generator_validate_input(
            ctx.as_context(),
            req,
        )
        .await
    }
}

// ── Tool operations ───────────────────────────────────────────────────────────

impl<TSys, S> OmniApi<TSys, S>
where
    TSys: ContextSys + GeneratorSys + Clone,
    S: OmniEventSubscriber,
{
    /// List all discovered tools in the workspace.
    pub async fn tool_list(
        &self,
    ) -> eyre::Result<crate::operations::tool::ToolListResponse> {
        let ctx = self.ctx.lock().await;
        crate::operations::tool::handle_tool_list(ctx.as_context()).await
    }

    /// Inspect a single tool, returning the JSON Schema of its own inputs.
    pub async fn tool_inspect(
        &self,
        name: &str,
    ) -> eyre::Result<crate::operations::tool::ToolInspectResponse> {
        let ctx = self.ctx.lock().await;
        crate::operations::tool::handle_tool_inspect(ctx.as_context(), name)
            .await
    }

    /// Run a tool by name with the given JSON arguments, returning its captured
    /// JSON value. `working_dir` selects the directory the tool operates in.
    pub async fn tool_run(
        &self,
        name: &str,
        args: serde_json::Value,
        working_dir: Option<crate::operations::tool::ToolWorkingDir>,
    ) -> eyre::Result<serde_json::Value>
    where
        TSys: bridge_rpc_services::FsSys + bridge_rpc_services::ProcSys,
        <TSys as system_traits::BaseFsMetadataAsync>::Metadata: Send,
    {
        let mut ctx = self.ctx.lock().await;
        ctx.load().await?;
        crate::operations::tool::handle_tool_run(
            ctx.as_loaded_context(),
            name,
            args,
            working_dir,
        )
        .await
    }
}

// ── Projection operations ───────────────────────────────────────────────────────

impl<TSys, S> OmniApi<TSys, S>
where
    TSys: crate::operations::projection::ProjectionSys,
    S: OmniEventSubscriber,
{
    /// Materialize every configured projection into the workspace, persisting
    /// the link ledger. Idempotent: unchanged links are left in place.
    pub async fn projection_sync(
        &self,
        req: crate::operations::projection::ProjectionSyncRequest,
    ) -> eyre::Result<crate::operations::projection::ProjectionSyncResponse>
    {
        let ctx = self.ctx.lock().await;
        crate::operations::projection::handle_projection_sync(
            ctx.as_context(),
            req,
        )
        .await
    }

    /// Report the state of every recorded projection link (read-only).
    pub async fn projection_status(
        &self,
        req: crate::operations::projection::ProjectionStatusRequest,
    ) -> eyre::Result<crate::operations::projection::ProjectionStatusResponse>
    {
        let ctx = self.ctx.lock().await;
        crate::operations::projection::handle_projection_status(
            ctx.as_context(),
            req,
        )
        .await
    }

    /// Tear down the links recorded for a single projection source `id`.
    pub async fn projection_unlink(
        &self,
        req: crate::operations::projection::ProjectionUnlinkRequest,
    ) -> eyre::Result<crate::operations::projection::ProjectionUnlinkResponse>
    {
        let ctx = self.ctx.lock().await;
        crate::operations::projection::handle_projection_unlink(
            ctx.as_context(),
            req,
        )
        .await
    }

    /// Remove ledger-recorded links whose destinations have become dangling.
    pub async fn projection_prune(
        &self,
        req: crate::operations::projection::ProjectionPruneRequest,
    ) -> eyre::Result<crate::operations::projection::ProjectionPruneResponse>
    {
        let ctx = self.ctx.lock().await;
        crate::operations::projection::handle_projection_prune(
            ctx.as_context(),
            req,
        )
        .await
    }
}
