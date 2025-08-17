use dir_walker::{
    DirWalker,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};

use crate::constants;

pub(crate) fn create_default_dir_walker() -> impl DirWalker {
    let cfg = IgnoreRealDirWalkerConfig {
        custom_ignore_filenames: vec![constants::OMNI_IGNORE.to_string()],
        standard_filters: true,
        overrides: None,
    };

    IgnoreRealDirWalker::new_with_config(cfg)
}
