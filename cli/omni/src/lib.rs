// The binary prints the final error and nothing else (RFC 0023).
#![allow(clippy::disallowed_macros)]

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser as _;
use omni_cli_core::{
    commands::{self, Cli, CliArgs, CliSubcommands, ErrorFormat},
    context::{self, Context, ContextError, get_root_dir},
};
use omni_diagnostic::DiagnosticReport;
use omni_tracing_subscriber::TracingConfig;
use scopeguard::defer;
use system_traits::impls::RealSys;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(all(not(feature = "dhat-heap"), not(target_os = "windows")))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn init_tracing(
    config: &omni_tracing_subscriber::TracingConfig,
) -> eyre::Result<()> {
    use omni_tracing_subscriber::TracingSubscriber;
    use tracing_subscriber::util::SubscriberInitExt;

    TracingSubscriber::new(config, vec![])?.try_init()?;

    Ok(())
}

#[inline(always)]
fn exit(code: ExitCode) -> ! {
    std::process::exit(if code == ExitCode::SUCCESS { 0 } else { 1 })
}

#[inline(always)]
#[allow(clippy::result_large_err)]
fn ctx(
    args: &CliArgs,
    tracing: &TracingConfig,
    ws_root_dir: Option<&Path>,
) -> Result<Context<RealSys>, ContextError> {
    tracing::trace!(?args, "cli_args_received");

    let sys = RealSys;
    if let Some(root) = ws_root_dir {
        context::from_args_root_dir_and_sys(args, root, sys, tracing)
    } else {
        context::from_args_and_sys(args, sys, tracing)
    }
}

pub async fn run(
    sc: &CliSubcommands,
    args: &CliArgs,
    tracing: &TracingConfig,
    ws_root_dir: Option<&Path>,
) -> eyre::Result<()> {
    #[allow(clippy::result_large_err)]
    let create_ctx = || ctx(args, tracing, ws_root_dir);

    match sc {
        CliSubcommands::Config(config) => {
            commands::config::run(config).await?;
        }
        CliSubcommands::Completion(completion) => {
            commands::completion::run(completion).await?;
        }
        CliSubcommands::Exec(exec) => {
            let context = create_ctx()?;
            let res = commands::exec::run(exec, &context, args.quiet).await?;
            exit(res);
        }
        CliSubcommands::Env(env) => {
            let context = create_ctx()?;
            commands::env::run(env, &context, args.quiet).await?;
        }
        CliSubcommands::Run(run) => {
            let context = create_ctx()?;
            let res = commands::run::run(run, &context, args.quiet).await?;
            exit(res);
        }
        CliSubcommands::Hash(hash_command) => {
            let context = create_ctx()?;
            commands::hash::run(hash_command, &context).await?;
        }
        CliSubcommands::Declspec(declspec_command) => {
            commands::declspec::run(declspec_command).await?;
        }
        CliSubcommands::Explain(cmd) => {
            commands::explain::run(cmd).await?;
        }
        CliSubcommands::Diagnostics(cmd) => {
            commands::diagnostics::run(cmd).await?;
        }
        CliSubcommands::Cache(cache_command) => {
            let context = create_ctx()?;
            commands::cache::run(cache_command, &context).await?;
        }
        CliSubcommands::Generator(command) => {
            let context = create_ctx()?;
            commands::generator::run(command, &context, args.quiet).await?;
        }
        CliSubcommands::Init(command) => {
            commands::init::run(command).await?;
        }
        CliSubcommands::Project(cmd) => {
            let context = create_ctx()?;
            commands::project::run(cmd, &context).await?;
        }
        CliSubcommands::Projection(cmd) => {
            let context = create_ctx()?;
            commands::projection::run(cmd, &context, args.quiet).await?;
        }
        CliSubcommands::Pack(cmd) => {
            let context = create_ctx()?;
            commands::pack::run(cmd, &context).await?;
        }
        CliSubcommands::Ignore(cmd) => {
            let context = create_ctx()?;
            let res = commands::ignore::run(cmd, &context).await?;
            exit(res);
        }
        CliSubcommands::Tool(cmd) => {
            let context = create_ctx()?;
            commands::tool::run(cmd, &context, args.quiet).await?;
        }
        CliSubcommands::RemoteSources(cmd) => {
            let context = create_ctx()?;
            let res = commands::remote_source::run(cmd, &context).await?;
            exit(res);
        }
        CliSubcommands::Mcp(mcp) => {
            if mcp.shared {
                return commands::mcp::run_shared(
                    context::workspace_init_config(args),
                    tracing,
                )
                .await;
            }
            let context = if let Some(root) = &mcp.root_dir {
                context::from_args_root_dir_and_sys(
                    args, root, RealSys, tracing,
                )?
            } else {
                create_ctx()?
            };
            commands::mcp::run(mcp, &context).await?;
        }
    }

    Ok(())
}

#[tracing::instrument(level = tracing::Level::DEBUG, err)]
async fn run_main() -> eyre::Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_location_section(cfg!(debug_assertions))
        .install()?;

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => report_usage_error(e),
    };
    let error_format = cli.args.error_format;

    match run_cli(cli).await {
        Ok(()) => Ok(()),
        Err(report) => report_failure(report, error_format),
    }
}

/// Whether JSON errors were asked for. Clap fails before the arguments are
/// parsed, so this looks at the raw arguments and the environment.
fn json_errors_requested() -> bool {
    // `args_os`: a non-Unicode argument must not panic the usage-error path.
    let mut args = std::env::args_os().skip(1);
    let mut requested = std::env::var_os("OMNI_ERROR_FORMAT");
    while let Some(a) = args.next() {
        if a == "--" {
            break;
        }
        if a == "--error-format" {
            requested = args.next();
        } else if let Some(v) =
            a.to_str().and_then(|a| a.strip_prefix("--error-format="))
        {
            requested = Some(v.into());
        }
    }
    requested.as_deref() == Some(std::ffi::OsStr::new("json"))
}

/// Clap exits on its own for `--help`/`--version`; usage errors are printed
/// as JSON when `--error-format json` was asked for, and exit `2` either way.
fn report_usage_error(e: clap::Error) -> ! {
    if e.use_stderr() && json_errors_requested() {
        eprintln!(
            "{}",
            serde_json::json!({
                "message": e.render().to_string().trim_end(),
                "context": Vec::<String>::new(),
            })
        );
        std::process::exit(2);
    }
    e.exit()
}

/// Prints a failure and exits for diagnostics (RFC 0021, section 4.3).
/// Errors that are not diagnostics keep the `color_eyre` report, unless JSON
/// output was asked for.
fn report_failure(
    report: eyre::Report,
    format: ErrorFormat,
) -> eyre::Result<()> {
    if let Some(d) = DiagnosticReport::from_report(&report) {
        // Fixed rule: credentials embedded in a URL are never printed.
        let d = d.redact_url_userinfo();
        match format {
            ErrorFormat::Text => eprint!("{}", d.to_text()),
            ErrorFormat::Json => eprintln!("{}", d.to_json()),
        }
        std::process::exit(d.exit_code());
    }

    if format == ErrorFormat::Json {
        let context = omni_diagnostic::render::chain_messages(&report);
        eprintln!(
            "{}",
            serde_json::json!({
                "message": format!("{report:#}"),
                "context": context,
            })
        );
        std::process::exit(1);
    }

    Err(report)
}

async fn run_cli(cli: Cli) -> eyre::Result<()> {
    let shared_mcp =
        matches!(&cli.subcommand, CliSubcommands::Mcp(cmd) if cmd.shared);

    let ws_root_dir = if cli.subcommand.is_mcp() {
        None
    } else {
        get_root_dir(&RealSys).ok()
    };

    let trace_file_path = cli.args.trace_file.clone().or_else(|| {
        (!shared_mcp).then(|| PathBuf::from("./.omni/trace/omni.log"))
    });

    let trace_file_path = trace_file_path.map(|path| {
        if !path.has_root()
            && let Some(ref root) = ws_root_dir
        {
            root.join(path)
        } else if shared_mcp && path.is_relative() {
            std::env::current_dir()
                .map(|dir| dir.join(&path))
                .unwrap_or(path)
        } else {
            path
        }
    });

    let file_level = if shared_mcp && trace_file_path.is_none() {
        omni_tracing_subscriber::Level::Off
    } else {
        cli.args.trace_file_level.value()
    };
    if cli.subcommand.is_mcp()
        && let Some(path) = &trace_file_path
        && commands::mcp::is_stdout_path(path)
    {
        eyre::bail!(
            "`--trace-file {}` resolves to stdout, which carries the MCP JSON-RPC stream; choose a regular file",
            path.display()
        );
    }

    let tracing_config = TracingConfig {
        stderr_filter: cli.args.trace.clone(),
        file_path: trace_file_path,
        file_level,
    };

    init_tracing(&tracing_config)?;
    tracing::trace!(?tracing_config, "tracing_initialized");

    omni_setup::initialize(omni_setup::InitConfig::builder().build())?;
    defer! {
        let res = omni_setup::deinitialize();

        if let Err(error) = res {
            tracing::error!(%error, "deinit_failed");
        }
    };

    run(
        &cli.subcommand,
        &cli.args,
        &tracing_config,
        ws_root_dir.as_deref(),
    )
    .await?;

    Ok(())
}

// The generator pipeline awaits its `run-generator` recursion on the thread
// that calls `block_on`, and each nested level keeps a suspended async frame
// live. Debug builds make those frames large, and the Windows process main
// thread has only about 1 MiB of stack, so a few levels of nesting overflow it.
// Run the runtime on a thread with a generous stack (and give tokio's own
// workers the same) so nesting depth is bounded by `max_depth`, not the OS.
const RUNTIME_STACK_SIZE: usize = 32 * 1024 * 1024;

fn run_runtime() -> eyre::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(RUNTIME_STACK_SIZE)
        .enable_all()
        .build()?;
    let result = rt.block_on(run_main());
    // micro-optimization to force blocking background tasks to be abondoned
    // instead of waiting for them to finish
    rt.shutdown_background();
    result
}

pub fn main() -> eyre::Result<()> {
    let worker = std::thread::Builder::new()
        .name("omni-main".to_string())
        .stack_size(RUNTIME_STACK_SIZE)
        .spawn(run_runtime)?;

    match worker.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
