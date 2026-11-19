use derive_new::new;
use strum::Display;

use clap::ArgAction;
use omni_remote_cache_core::routes::root::RouterConfig;

use crate::{
    local_disk_backend::LocalDiskBackendConfig, s3_backend::S3BackendConfig,
};

#[derive(clap::Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub subcommand: CliSubcommands,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum CliSubcommands {
    Serve(#[command(flatten)] ServeCommand),
}

#[derive(clap::Args, Debug, Clone)]
pub struct ServeCommand {
    #[command(flatten)]
    pub args: ServeArgs,
}

#[derive(clap::Args, Clone, Debug, new)]
#[allow(clippy::too_many_arguments)]
pub struct ServeArgs {
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
        default_value = "orcs.config.json",
        help = "Path to the configuration file",
        env = "OMNI_REMOTE_CACHE_SERVICE_CONFIG"
    )]
    pub config: Option<String>,

    #[clap(
        long,
        short = 't',
        default_value = "file",
        help = "How to interpret the configuration",
        env = "OMNI_REMOTE_CACHE_SERVICE_CONFIG_TYPE"
    )]
    pub config_type: Option<ConfigType>,

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

    #[command(flatten)]
    pub routes: Option<RouterArgs>,

    #[arg(
        long,
        env = "OMNI_REMOTE_CACHE_SERVICE_REPOSITORY_CONFIG",
        help = "Path to a JSON file that reads ownership data and API keys from a SQL database using your own queries, instead of the configuration file"
    )]
    pub repository_config: Option<String>,

    #[arg(
        long,
        default_value_t = omni_remote_cache_core::state::DEFAULT_MAX_ARTIFACT_SIZE,
        env = "OMNI_REMOTE_CACHE_SERVICE_MAX_ARTIFACT_SIZE",
        help = "The largest artifact accepted, in bytes. Larger uploads get 413 and are not stored"
    )]
    #[new(value = "omni_remote_cache_core::state::DEFAULT_MAX_ARTIFACT_SIZE")]
    pub max_artifact_size: u64,

    #[arg(
        long,
        default_value_t = 7,
        env = "OMNI_REMOTE_CACHE_SERVICE_ISOLATED_TTL_DAYS",
        help = "Remove isolated entries stored more than this many days ago (local-disk and memory backends only; S3 and R2 use a bucket lifecycle rule on the `isolated/` prefix). 0 keeps them forever. Shared entries never expire"
    )]
    #[new(value = "7")]
    pub isolated_ttl_days: u64,
}

#[derive(
    clap::ValueEnum,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Display,
)]
pub enum BackendType {
    #[strum(serialize = "s3")]
    S3,
    #[strum(serialize = "local-disk")]
    LocalDisk,
    #[strum(serialize = "memory")]
    InMemory,
}

#[derive(
    clap::ValueEnum,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Display,
)]
pub enum ConfigType {
    #[strum(serialize = "inline")]
    Inline,
    #[strum(serialize = "file")]
    File,
}

#[derive(Default, clap::Args, Debug, Clone, new)]
pub struct RouterArgs {
    #[clap(
        long = "routes.api-prefix",
        env = "OMNI_REMOTE_CACHE_SERVICE_ROUTES_API_PREFIX",
        default_value = "/api"
    )]
    pub api_prefix: Option<String>,

    #[clap(
        long = "routes.enable-openapi",
        env = "OMNI_REMOTE_CACHE_SERVICE_ROUTES_ENABLE_OPENAPI",
        action = ArgAction::SetTrue,
        default_value = "false"
    )]
    pub enable_openapi: bool,
}

impl From<RouterArgs> for RouterConfig {
    fn from(args: RouterArgs) -> Self {
        RouterConfig::new(args.api_prefix, args.enable_openapi)
    }
}
