use std::path::PathBuf;

use bon::Builder;

#[derive(Debug, Builder)]
pub struct RemoteSourceConfig {
    #[builder(into)]
    pub lockfile_path: PathBuf,

    #[builder(into)]
    pub soure_dir_path: PathBuf,
}
