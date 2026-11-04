//! Notices raised by the task executor (RFC 0023).

use omni_notice::Notice;

omni_codes::prefixes!("redact");

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
