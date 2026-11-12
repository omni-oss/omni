use std::sync::Arc;

use crate::{
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    hooks::{Background, DynArtifactHooks, HookSettings, NoopHooks},
    security::DynSecurityService,
    services::DynValidationService,
};

pub trait DependencyProvider: Send + Sync + 'static {
    fn workspace_repository(&self) -> DynWorkspaceRepository;
    fn organization_repository(&self) -> DynOrganizationRepository;
    fn environment_repository(&self) -> DynEnvironmentRepository;
    fn tenant_repository(&self) -> DynTenantRepository;
    fn validation_service(&self) -> DynValidationService;
    fn security_service(&self) -> DynSecurityService;

    /// Veto-capable hooks. No-op unless overridden.
    fn hooks(&self) -> DynArtifactHooks {
        Arc::new(NoopHooks)
    }

    fn hook_settings(&self) -> HookSettings {
        HookSettings::default()
    }

    /// Post-response task runner and timer. Without one, `after` hooks run
    /// inline and hook timeouts are not enforced.
    fn background(&self) -> Option<Arc<dyn Background>> {
        None
    }
}
