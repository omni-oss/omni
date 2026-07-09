use dir_walker::{
    DirWalker,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};

use crate::constants;

pub fn create_default_dir_walker() -> impl DirWalker {
    let cfg = IgnoreRealDirWalkerConfig::builder()
        .custom_ignore_filenames(vec![constants::OMNI_IGNORE.to_string()])
        .build();

    IgnoreRealDirWalker::new_with_config(cfg)
}
