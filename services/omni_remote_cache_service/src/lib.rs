#![allow(clippy::redundant_field_names)]

pub mod args;
mod build;
mod init_tracing;
pub mod local_disk_backend;
pub mod repository;
pub mod s3_backend;
pub mod state;
pub mod storage_backend;

pub use omni_remote_cache_core::{
    config, data, data_impl, entry, hooks, providers, request, response,
    routes, security, services,
};

use clap::Parser;
use omni_remote_cache_core::routes::root::RouterConfig;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};

use crate::{
    args::{Cli, CliSubcommands},
    init_tracing::init_tracing,
    state::build_state,
};

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    init_tracing()?;

    let cli = Cli::parse();

    match cli.subcommand {
        CliSubcommands::Serve(serve) => {
            let state = build_state(&serve.args).await?;
            let storage = state.storage_backend.clone();

            let routing_config: RouterConfig =
                serve.args.routes.clone().unwrap_or_default().into();
            let router = routes::root::build_router(&routing_config)
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(CatchPanicLayer::new())
                        .layer(TraceLayer::new_for_http()),
                );

            if serve.args.isolated_ttl_days > 0
                && serve.args.backend != args::BackendType::S3
            {
                spawn_isolated_sweeper(storage, serve.args.isolated_ttl_days);
            }

            let socket = TcpListener::bind(&serve.args.listen).await?;

            tracing::info!("Listening on {}", socket.local_addr()?);

            axum::serve(socket, router).await?;
        }
    }

    Ok(())
}

/// Removes expired isolated entries now and then. Each pass is bounded so a big
/// store is worked through over several passes, and a failed pass only logs.
fn spawn_isolated_sweeper(
    storage: std::sync::Arc<omni_remote_cache_core::state::DynStorageBackend>,
    ttl_days: u64,
) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(3600);
    const MAX_OBJECTS_PER_PASS: u64 = 100_000;
    tokio::spawn(async move {
        let mut cursor: Option<String> = None;
        loop {
            let ttl = time::Duration::days(ttl_days.min(36_500) as i64);
            let cutoff = time::OffsetDateTime::now_utc() - ttl;
            match omni_remote_cache_core::sweeper::sweep_isolated(
                storage.as_ref(),
                cutoff,
                Some(MAX_OBJECTS_PER_PASS),
                cursor.take(),
            )
            .await
            {
                Ok(report) => {
                    tracing::info!(
                        scanned = report.scanned,
                        deleted = report.deleted,
                        skipped = report.skipped,
                        failed = report.failed,
                        resuming = report.next_cursor.is_some(),
                        "isolated entry sweep finished"
                    );
                    cursor = report.next_cursor;
                }
                Err(error) => {
                    tracing::warn!(%error, "isolated entry sweep failed")
                }
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}
