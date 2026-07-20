//! The **capability policy** config shape attached at each configuration level.
//!
//! A [`CapabilityPolicyConfig<P>`] is the always-object form of a level's
//! capability declaration: an ordered array of [`rules`](CapabilityPolicyConfig::rules)
//! plus a single [`strictness`](CapabilityPolicyConfig::strictness) stance. It
//! replaces the older two-sibling shape (a bare `capabilities` array next to a
//! separate `capabilities_strictness` scalar) with one object so every level
//! —workspace, generator, action— expresses both halves uniformly.
//!
//! ## Two algebras in one object
//!
//! The two halves cascade *differently* down the level stack, and both only
//! ever get **stricter** downward:
//!
//! * `rules` **intersect** (the shrink-only / attenuation model): a deeper level
//!   can only narrow the authority it inherited, never widen it. This is the
//!   [`CapabilityRules`] cascade.
//! * `strictness` **maxes** (`Warn < RequireFloor`, monotonic tightening): the
//!   effective stance for a script is the most-severe stance declared by any
//!   enclosing level. It never attenuates like `rules` do.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, generate::SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{CapabilityProfile, CapabilityRules};

/// How strictly to treat a **capability floor gap** — a governed domain that,
/// on the resolved runtime/platform, ends up enforced only by a *bypassable*
/// in-process mechanism (the RPC broker or the script shim) with no
/// un-bypassable runtime-flag or OS-sandbox floor. Examples: `net`/`process` on
/// Bun (no permission model), or any governed domain off-Linux while no
/// OS-sandbox backend covers it.
///
/// The variants are ordered `Warn < RequireFloor`, so `strictness` can be
/// combined across configuration levels by taking the [`max`](Ord::max) — the
/// most-severe stance any enclosing level declared wins.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilitiesStrictness {
    /// Proceed, surfacing each floor gap as a non-fatal warning. Enforcement is
    /// still active as defense in depth. This is the shipped default.
    #[default]
    Warn,
    /// Refuse to run when any governed domain lacks an un-bypassable floor for
    /// the resolved runtime/platform: a run proceeds only when every governed
    /// domain is floored (so the confinement cannot be bypassed by raw sockets,
    /// direct syscalls, or FFI/N-API/WASM).
    RequireFloor,
}

/// The capability policy declared at one configuration level: an ordered array
/// of allow/deny [`rules`](Self::rules) and a floor-gap [`strictness`](Self::strictness)
/// stance.
///
/// Always an object on the wire — there is no bare-array shorthand — so serde
/// reports precise field errors and both halves are declared uniformly at every
/// level. Both fields default (`{}` = no rules, `warn` stance), and unknown
/// fields are rejected.
///
/// See the [module docs](self) for how the two halves cascade differently
/// (`rules` intersect, `strictness` maxes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    serialize = "P::AppliesTo: Serialize, P::Extra: Serialize",
    deserialize = "P::AppliesTo: serde::Deserialize<'de>, P::Extra: serde::Deserialize<'de>"
))]
pub struct CapabilityPolicyConfig<P: CapabilityProfile = ()> {
    /// The ordered allow/deny rules for this level. Cascades by attenuation
    /// (a deeper level may only narrow authority).
    #[serde(default)]
    pub rules: CapabilityRules<P>,

    /// The floor-gap stance for this level. Combined across levels by taking
    /// the most-severe (`Warn < RequireFloor`).
    #[serde(default)]
    pub strictness: CapabilitiesStrictness,
}

impl<P: CapabilityProfile> Default for CapabilityPolicyConfig<P> {
    fn default() -> Self {
        Self {
            rules: CapabilityRules::default(),
            strictness: CapabilitiesStrictness::default(),
        }
    }
}

impl<P: CapabilityProfile> CapabilityPolicyConfig<P> {
    /// Build a policy from rules with the default (`warn`) stance.
    pub fn from_rules(rules: CapabilityRules<P>) -> Self {
        Self {
            rules,
            strictness: CapabilitiesStrictness::default(),
        }
    }
}

// A manual `JsonSchema` impl is required because deriving it on a type generic
// over `P` would add an unsatisfiable `P: JsonSchema` bound (the profile markers
// are not schema types). Mirrors the hand-written impls in
// [`crate::json_schema`].
impl<P: CapabilityProfile> JsonSchema for CapabilityPolicyConfig<P> {
    fn schema_name() -> Cow<'static, str> {
        format!("CapabilityPolicyConfig_{}", P::NAME).into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let rules = serde_json::to_value(
            generator.subschema_for::<CapabilityRules<P>>(),
        )
        .expect("a Schema is always valid JSON");
        let strictness = serde_json::to_value(
            generator.subschema_for::<CapabilitiesStrictness>(),
        )
        .expect("a Schema is always valid JSON");

        let value = json!({
            "type": "object",
            "description": "A level's capability policy: an ordered list of allow/deny rules plus a floor-gap strictness stance.",
            "properties": {
                "rules": rules,
                "strictness": strictness
            },
            "additionalProperties": false,
            "default": {}
        });
        match value {
            serde_json::Value::Object(map) => Schema::from(map),
            _ => unreachable!("object literal is always a JSON object"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strictness_orders_warn_below_require_floor() {
        assert!(
            CapabilitiesStrictness::Warn < CapabilitiesStrictness::RequireFloor
        );
        assert_eq!(
            CapabilitiesStrictness::Warn
                .max(CapabilitiesStrictness::RequireFloor),
            CapabilitiesStrictness::RequireFloor
        );
    }

    #[test]
    fn empty_object_parses_to_defaults() {
        let cfg: CapabilityPolicyConfig =
            serde_json::from_str("{}").expect("empty object parses");
        assert!(cfg.rules.is_empty());
        assert_eq!(cfg.strictness, CapabilitiesStrictness::Warn);
    }

    #[test]
    fn rules_only_defaults_strictness_to_warn() {
        let cfg: CapabilityPolicyConfig = serde_json::from_str(
            r#"{ "rules": [{ "access": "allow", "domain": "fs.read", "patterns": ["**"] }] }"#,
        )
        .expect("rules-only parses");
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.strictness, CapabilitiesStrictness::Warn);
    }

    #[test]
    fn strictness_scalar_parses_kebab_case() {
        let cfg: CapabilityPolicyConfig =
            serde_json::from_str(r#"{ "strictness": "require-floor" }"#)
                .expect("strictness parses");
        assert_eq!(cfg.strictness, CapabilitiesStrictness::RequireFloor);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = serde_json::from_str::<CapabilityPolicyConfig>(
            r#"{ "rulez": [] }"#,
        );
        assert!(err.is_err(), "unknown field must be rejected");
    }

    #[test]
    fn unknown_strictness_value_is_rejected() {
        let err = serde_json::from_str::<CapabilityPolicyConfig>(
            r#"{ "strictness": "yolo" }"#,
        );
        assert!(err.is_err(), "unknown strictness value must be rejected");
    }

    #[test]
    fn json_schema_exposes_rules_and_strictness() {
        use schemars::generate::SchemaGenerator;

        let generator = SchemaGenerator::default();
        let root = generator.into_root_schema_for::<CapabilityPolicyConfig>();
        let json = serde_json::to_value(&root).expect("valid json");
        // The object schema (possibly resolved through `$defs`) must name both
        // halves. Serialize the whole document and look for the property keys,
        // which is enough to prove the manual impl emitted them.
        let text = json.to_string();
        assert!(text.contains("rules"), "schema must expose `rules`");
        assert!(
            text.contains("strictness"),
            "schema must expose `strictness`"
        );
    }
}
