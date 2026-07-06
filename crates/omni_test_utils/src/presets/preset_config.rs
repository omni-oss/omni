use crate::HarnessConfig;

/// Back-compat alias. Presets are now just [`HarnessConfig`] values, so any
/// `PresetConfig` is a full harness configuration.
pub type PresetConfig = HarnessConfig;
