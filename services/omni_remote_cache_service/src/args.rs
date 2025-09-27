use crate::{
    local_disk_backend::LocalDiskBackendConfig, s3_backend::S3BackendConfig,
};

#[derive(clap::Parser)]
pub struct Cli {
    #[command(flatten)]
    pub args: CliArgs,
}

#[derive(clap::Args)]
pub struct CliArgs {
    #[clap(
        long,
        short,
        default_value = "0.0.0.0:3000",
        env = "OMNI_REMOTE_CACHE_SERVICE_LISTEN",
        help = "The address to listen on"
    )]
    pub listen: String,

    #[clap(
        long,
        short,
        default_value = "false",
        env = "OMNI_REMOTE_CACHE_SERVICE_SECURE",
        help = "Whether to use TLS for the server"
    )]
    pub secure: bool,

    #[command(flatten)]
    pub s3: Option<S3BackendConfig>,

    #[command(flatten)]
    pub local_disk: Option<LocalDiskBackendConfig>,

    #[arg(
        long,
        default_value = "100",
        help = "The maximum number of items to keep in the cache"
    )]
    pub lru_cache_capacity: Option<usize>,

    #[arg(
        long,
        short,
        default_value = "memory",
        value_enum,
        env = "OMNI_REMOTE_CACHE_SERVICE_BACKEND",
        help = "The backend to use for storing artifacts"
    )]
    pub backend: BackendType,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum BackendType {
    S3,
    LocalDisk,
    Memory,
}
