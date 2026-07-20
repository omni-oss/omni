//! [`NoSandbox`]: a backend that enforces nothing.
//!
//! It exists so that "no enforcement mechanism is available here" is an
//! explicit, first-class value rather than an absence. Because its
//! [`Coverage`] is empty, composing a plan with only `NoSandbox` makes every
//! restricted domain a coverage gap, so [`crate::require_full_coverage`] fails
//! closed — exactly the safe default for an unknown or unsupported target.

use omni_capabilities::RequiredCapabilities;

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError,
    PatternResolver, Tier,
};

/// A backend that provides no confinement whatsoever.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSandbox;

impl EnforcementBackend for NoSandbox {
    fn name(&self) -> &'static str {
        "no-sandbox"
    }

    fn tier(&self) -> Tier {
        // It nominally sits at the OS layer (the place a real sandbox would
        // go), but covers nothing.
        Tier::OsSandbox
    }

    fn coverage(&self) -> Coverage {
        Coverage::none()
    }

    fn plan(
        &self,
        _req: &RequiredCapabilities,
        _roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        Ok(BackendPlan::new())
    }
}
