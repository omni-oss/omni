//! Notices raised by the generator (RFC 0023).

use omni_notice::Notice;

omni_codes::prefixes!("runtime");

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
