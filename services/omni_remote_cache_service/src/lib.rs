mod args;
mod local_disk_backend;
mod routes;
mod s3_backend;
mod state;

use clap::Parser;
use tokio::net::TcpListener;

use crate::args::Cli;

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let router = routes::root::build_router();

    let socket = TcpListener::bind(&cli.args.listen).await?;

    axum::serve(socket, router).await?;

    Ok(())
}
