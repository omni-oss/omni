use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, clap::Args, Clone, PartialEq, Eq)]
pub struct LocalDiskBackendConfig {
    #[arg(
        long = "local-disk.root-dir",
        default_value = "./data",
        env = "OMNI_REMOTE_CACHE_SERVICE_LOCAL_DISK_ROOT_DIR",
        help = "The root directory to use for local disk storage"
    )]
    pub root_dir: PathBuf,
}
