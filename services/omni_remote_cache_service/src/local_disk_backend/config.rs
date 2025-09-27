use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, clap::Args, Clone, PartialEq, Eq)]
pub struct LocalDiskBackendConfig {
    #[arg(
        long = "local-disk.root_dir",
        default_value = "./data",
        env = "OMNI_REMOTE_CACHE_SERVICE_LOCAL_DISK_ROOT_DIR",
        help = "The root directory to use for local disk storage"
    )]
    pub root_dir: String,

    #[arg(
        long = "local-disk.default-subdir",
        default_value = "default",
        env = "OMNI_REMOTE_CACHE_SERVICE_LOCAL_DISK_DEFAULT_SUBDIR",
        help = "The default subdirectory to use for local disk storage if none is specified"
    )]
    pub default_subdir: String,
}
