/// RAII guard that calls `omni_setup::deinitialize()` on drop.
///
/// Created by [`OmniApiBuilder::build`] when `with_setup == true`.
pub(crate) struct SetupGuard;

impl Drop for SetupGuard {
    fn drop(&mut self) {
        let _ = omni_setup::deinitialize();
    }
}
