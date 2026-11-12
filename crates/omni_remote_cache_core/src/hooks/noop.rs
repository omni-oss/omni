use async_trait::async_trait;

use super::{ArtifactHooks, HookContext, HookDecision, HookError, HookOutcome};

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHooks;

#[async_trait]
impl ArtifactHooks for NoopHooks {
    async fn before(&self, _: &HookContext) -> Result<HookDecision, HookError> {
        Ok(HookDecision::Allow)
    }

    async fn after(
        &self,
        _: &HookContext,
        _: &HookOutcome,
    ) -> Result<(), HookError> {
        Ok(())
    }
}
