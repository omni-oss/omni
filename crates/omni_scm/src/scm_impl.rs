use derive_new::new;
use strum::{Display, EnumDiscriminants, EnumIs, VariantArray};

use crate::{Scm, git::Git};

#[derive(EnumDiscriminants, new)]
#[strum_discriminants(
    vis(pub),
    name(SupportedScm),
    derive(PartialOrd, Ord, EnumIs, Display, VariantArray)
)]
pub enum ScmImplementation {
    Git(Git),
}

impl Scm for ScmImplementation {
    #[inline(always)]
    fn changed_files(
        &self,
        base: &str,
        target: &str,
    ) -> Result<Vec<std::path::PathBuf>, crate::error::Error> {
        match self {
            ScmImplementation::Git(git) => git.changed_files(base, target),
        }
    }

    #[inline(always)]
    fn default_base(&self) -> &str {
        match self {
            ScmImplementation::Git(git) => git.default_base(),
        }
    }

    #[inline(always)]
    fn default_target(&self) -> &str {
        match self {
            ScmImplementation::Git(git) => git.default_target(),
        }
    }
}
