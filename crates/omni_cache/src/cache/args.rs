use bytesize::ByteSize;
use derive_new::new;

#[derive(Debug, Clone, PartialEq, Eq, new, Default)]
pub struct PruneCacheArgs<'a> {
    pub dry_run: bool,
    pub stale_only: PruneStaleOnly,
    pub older_than: Option<std::time::Duration>,
    pub project_name_glob: Option<&'a str>,
    pub task_name_glob: Option<&'a str>,
    pub larger_than: Option<ByteSize>,
}

#[derive(Debug, Clone, PartialEq, Eq, new, Default)]
pub enum PruneStaleOnly {
    #[default]
    Off,
    On {},
}
