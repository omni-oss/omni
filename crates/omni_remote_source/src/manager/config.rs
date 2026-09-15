use std::path::PathBuf;

use bon::Builder;

#[derive(Debug, Builder)]
pub struct RemoteSourceConfig {
    #[builder(into)]
    pub lockfile_path: PathBuf,

    #[builder(into)]
    pub store_root_path: PathBuf,
}
