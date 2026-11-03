#![allow(clippy::redundant_field_names)]

pub mod args;
mod build;
mod init_tracing;
pub mod local_disk_backend;
pub mod s3_backend;
pub mod state;
pub mod storage_backend;

pub use omni_remote_cache_core::{
    config, data, data_impl, providers, request, response, routes, services,
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

            let routing_config: RouterConfig =
                serve.args.routes.clone().unwrap_or_default().into();
            let router = routes::root::build_router(&routing_config)
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(CatchPanicLayer::new())
                        .layer(TraceLayer::new_for_http()),
                );

            let socket = TcpListener::bind(&serve.args.listen).await?;

            log::info!("Listening on {}", socket.local_addr()?);

            axum::serve(socket, router).await?;
        }
    }

    Ok(())
}
