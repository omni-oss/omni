mod args;
mod build;
mod config;
mod data;
mod data_impl;
mod init_tracing;
mod local_disk_backend;
mod providers;
mod request;
mod response;
mod routes;
mod s3_backend;
mod scalar;
mod services;
mod state;
mod storage_backend;
mod utils;

use clap::Parser;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};

use crate::{args::Cli, init_tracing::init_tracing, state::ServiceState};

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    init_tracing()?;

    let cli = Cli::parse();

    match cli.subcommand {
        args::CliSubcommands::Serve(serve) => {
            let state = ServiceState::from_args(&serve.args).await;

            let routing_config = serve.args.routes.unwrap_or_default();
            let router = routes::root::build_router(&routing_config)
                .with_state(state)
                .layer(
                    ServiceBuilder::new()
                        .layer(CatchPanicLayer::new())
                        .layer(TraceLayer::new_for_http()),
                );

            let socket = TcpListener::bind(&serve.args.listen).await?;

            trace::info!("Listening on {}", socket.local_addr()?);

            axum::serve(socket, router).await?;
        }
    }

    Ok(())
}
