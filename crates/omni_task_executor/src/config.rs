use derive_builder::Builder;
use getset::{CloneGetters, CopyGetters};

use crate::{OnFailure, call::Call};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Builder,
    CloneGetters,
    CopyGetters,
)]
#[builder(setter(into, strip_option))]
pub struct ExecutionConfig {
    /// if true, it will run all tasks ignoring the dependency graph
    #[getset(get_copy = "pub")]
    ignore_dependencies: bool,

    /// Glob pattern to filter the projects
    #[builder(default)]
    #[getset(get = "pub")]
    project_filter: Option<String>,

    // Filter the projects/tasks based on the meta configuration
    #[builder(default)]
    #[getset(get = "pub")]
    meta_filter: Option<String>,

    /// if true, it will not consider the cache and will always execute the task
    #[builder(default = false)]
    #[getset(get_copy = "pub")]
    force: bool,

    /// if true, it will not cache the execution result, future runs will not see the cached result
    #[builder(default = false)]
    #[getset(get_copy = "pub")]
    no_cache: bool,

    /// How to handle failures
    #[builder(default = OnFailure::SkipDependents)]
    #[getset(get_copy = "pub")]
    on_failure: OnFailure,

    #[builder(default = false)]
    #[getset(get_copy = "pub")]
    dry_run: bool,

    #[builder(default = true)]
    #[getset(get_copy = "pub")]
    replay_cached_logs: bool,

    #[builder(default)]
    #[getset(get_copy = "pub")]
    max_concurrency: Option<usize>,

    #[builder(default)]
    #[getset(get_copy = "pub")]
    add_task_details: bool,

    #[builder(setter(custom))]
    #[getset(get = "pub")]
    call: Call,
}

impl ExecutionConfigBuilder {
    pub fn call(&mut self, call: impl Into<Call>) -> &mut Self {
        let call: Call = call.into();

        // default handling for commands is to run them with no dependencies and never consider the cache
        if matches!(call, Call::Command { .. }) {
            if self.ignore_dependencies.is_none() {
                self.ignore_dependencies = Some(true);
            }

            if self.force.is_none() {
                self.force = Some(true);
            }

            if self.no_cache.is_none() {
                self.no_cache = Some(true);
            }

            if self.on_failure.is_none() {
                self.on_failure = Some(OnFailure::Continue);
            }
        }

        self.call = Some(call);

        self
    }
}

impl ExecutionConfig {
    pub fn should_replay_logs(&self) -> bool {
        !self.dry_run && self.replay_cached_logs
    }
}
