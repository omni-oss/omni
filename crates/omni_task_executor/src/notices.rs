//! Notices raised by the task executor.

use omni_notice::Notice;

omni_codes::prefixes!("redact", "cache");

/// Redaction is disabled for this run.
///
/// The run was started with `--redact none`, so values declared sensitive
/// (RFC 0022) are not masked and may appear in task output, logs and dry-run
/// command lines.
///
/// Fix: remove `--redact none`, or acknowledge that sensitive values may be
/// printed.
#[derive(Notice)]
#[notice(
    code = "redact.disabled",
    level = warn,
    essential,
    message = "Redaction is disabled (--redact none): sensitive values may appear in output"
)]
pub struct RedactDisabled;

/// Cache-enabled tasks ran without cache because a dependency is uncached.
///
/// A cache-enabled task is reuse-eligible only when every execution dependency
/// is cache-eligible, since an uncached dependency has no digest to key on.
/// Affected tasks execute normally, with no lookup, replay or persistence.
///
/// Fix: enable caching on the uncached dependencies, or accept that their
/// dependents run uncached. Per-task details are in the dry-run output and
/// each task result's cache status.
#[derive(Notice)]
#[notice(
    code = "cache.dependency_demoted",
    level = info,
    message = "{count} cache-enabled task(s) ran uncached because a dependency is not cache-eligible"
)]
pub struct CacheDependencyDemoted {
    pub count: usize,
}

/// A task opted out of remote caching, but dependencies were disabled or absent
/// from this run. The opt-out does not verify the origin of existing files.
///
/// Fix: include and enable the dependencies, and use `--no-cache` if the task
/// needs freshly built outputs rather than local cache hits.
#[derive(Notice)]
#[notice(
    code = "cache.remote_opt_out_skipped_dependencies",
    level = warn,
    message = "{task} has cache.remote: false, but these dependencies were disabled or not scheduled: {dependencies}. Existing outputs are left in place; their origin is not verified."
)]
pub struct RemoteOptOutSkippedDependencies {
    pub task: String,
    pub dependencies: String,
}
