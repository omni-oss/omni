//! Capability and sandbox notices.

use omni_notice::Notice;

omni_codes::prefixes!("sandbox", "capabilities", "runtime");

/// OS-level sandbox is disabled.
///
/// `OMNI_DISABLE_OS_SANDBOX` is set, so tasks and scripts run without
/// kernel-level isolation. Filesystem confinement falls back to the
/// bypassable in-process broker.
///
/// Fix: unset `OMNI_DISABLE_OS_SANDBOX` to restore OS-level confinement.
#[derive(Notice)]
#[notice(
    code = "sandbox.os_disabled",
    level = warn,
    essential,
    message = "OS-level sandbox is DISABLED for this run (OMNI_DISABLE_OS_SANDBOX is set): filesystem confinement falls back to the bypassable in-process broker and the kernel backstop against direct syscalls is dropped. This is a security downgrade; unset OMNI_DISABLE_OS_SANDBOX to restore OS-level confinement."
)]
pub struct SandboxOsDisabled;

/// A capability policy is not fully enforced.
///
/// A rule that opted into `on_unenforceable: warn` ran with strictly less
/// confinement than requested.
#[derive(Notice)]
#[notice(
    code = "capabilities.not_enforced",
    level = warn,
    message = "capability policy not fully enforced: {warning}"
)]
pub struct CapabilitiesNotEnforced {
    pub warning: String,
}

/// A capability is enforced without an un-bypassable floor.
///
/// A governed domain is enforced only by the bypassable in-process
/// broker/shim, with no runtime-flag or OS-sandbox floor for the resolved
/// runtime.
#[derive(Notice)]
#[notice(
    code = "capabilities.no_floor",
    level = warn,
    message = "capability enforced without an un-bypassable floor: {reason}"
)]
pub struct CapabilitiesNoFloor {
    pub reason: String,
}

/// A capability policy was declared but is ignored.
///
/// The capabilities feature is experimental and disabled, so scripts run
/// unconfined.
///
/// Fix: enable it with `enable_experimental: true` (or
/// `enable_experimental: { capabilities: true }`) in the workspace
/// configuration.
#[derive(Notice)]
#[notice(
    code = "capabilities.ignored",
    level = warn,
    message = "a capability policy is declared but the capabilities feature is experimental and disabled; it is ignored and scripts run unconfined; enable it with `enable_experimental: true` (or `enable_experimental: {{ capabilities: true }}`) in the workspace configuration"
)]
pub struct CapabilitiesIgnored;

/// The import scan reported a note.
///
/// The scan that derives a script's read scope could not fully resolve its
/// imports; `note` says what.
#[derive(Notice)]
#[notice(
    code = "capabilities.import_scan",
    level = warn,
    message = "import-scan: {note}"
)]
pub struct CapabilitiesImportScan {
    pub note: String,
}

/// An experimental JavaScript runtime is in use.
///
/// Only `deno` is fully supported. `node` and `bun` are not fully sandboxed on
/// every platform. `how` says whether the runtime was auto-detected or chosen
/// explicitly.
///
/// Fix: install or select `deno` for enforced capability confinement.
#[derive(Notice)]
#[notice(
    code = "runtime.experimental",
    level = warn,
    message = "{how} the `{runtime}` runtime, which is experimental and not fully sandboxed on every platform; only `deno` is fully supported. Install/select `deno` for enforced capability confinement."
)]
pub struct RuntimeExperimental {
    pub how: String,
    pub runtime: String,
}
