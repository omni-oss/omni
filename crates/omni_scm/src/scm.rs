use std::path::PathBuf;

use crate::error::Error;

pub trait Scm {
    fn changed_files(
        &self,
        base: &str,
        target: &str,
    ) -> Result<Vec<PathBuf>, Error>;

    fn default_base(&self) -> &str;
    fn default_target(&self) -> &str;
}
