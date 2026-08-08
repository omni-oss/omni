//! Opt-in gating of experimental / in-progress omni features.
//!
//! The single type here, [`ExperimentalFeatures`], is a transparent wrapper
//! over [`AllOrKeyed<bool>`] so a workspace can flip every experimental feature
//! at once (a bare boolean) or toggle them individually by name (a per-feature
//! map). It lives in its own crate so any subsystem can consult the feature
//! switches without depending on the full configuration crate, and so the set
//! of known feature keys has one home as it grows.

use omni_config_types::AllOrKeyed;

/// Opt-in switch for experimental / in-progress features.
///
/// A transparent wrapper over [`AllOrKeyed<bool>`], so it deserializes from
/// either a bare boolean (enable/disable *every* feature) or a per-feature map:
///
/// ```yaml
/// enable_experimental: true          # every experimental feature
/// # or
/// enable_experimental:
///   capabilities: true               # only the named feature(s)
/// ```
///
/// Off by default. Experimental features are not covered by stability
/// guarantees and may change or be removed without notice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "schemars", schemars(transparent))]
pub struct ExperimentalFeatures(pub AllOrKeyed<bool>);

impl ExperimentalFeatures {
    /// The `capabilities` feature key (capability-based sandboxing and
    /// enforcement of generator scripts).
    pub const CAPABILITIES: &'static str = "capabilities";

    /// Whether the named experimental feature is enabled. The boolean form
    /// applies to every feature; the map form enables only the keys set to
    /// `true` (an absent key is disabled).
    pub fn is_enabled(&self, feature: &str) -> bool {
        self.0.get(feature).copied().unwrap_or(false)
    }

    /// Whether the experimental capabilities subsystem is enabled.
    pub fn capabilities(&self) -> bool {
        self.is_enabled(Self::CAPABILITIES)
    }
}

impl From<bool> for ExperimentalFeatures {
    fn from(value: bool) -> Self {
        ExperimentalFeatures(AllOrKeyed::All(value))
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_every_feature_disabled() {
        let features = ExperimentalFeatures::default();
        assert!(!features.capabilities());
        assert!(!features.is_enabled("anything"));
    }

    #[test]
    fn bool_form_toggles_every_feature() {
        let features: ExperimentalFeatures =
            serde_json::from_str("true").unwrap();
        assert!(features.capabilities());
        assert!(features.is_enabled("anything-else"));
    }

    #[test]
    fn per_feature_form_toggles_named_features() {
        let features: ExperimentalFeatures =
            serde_json::from_str(r#"{"capabilities": true}"#).unwrap();
        assert!(features.capabilities());
        assert!(!features.is_enabled("other"));
    }

    #[test]
    fn per_feature_false_disables() {
        let features: ExperimentalFeatures =
            serde_json::from_str(r#"{"capabilities": false}"#).unwrap();
        assert!(!features.capabilities());
    }
}
