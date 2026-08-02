//! # `omni_capabilities`
//!
//! The **core, platform-neutral capability policy engine** for omni.
//!
//! This crate answers one question: *is a given operation allowed?* It is a
//! pure, deterministic policy model — it performs no I/O and deliberately knows
//! **nothing** about operating systems, sandbox mechanisms (Landlock, Seatbelt,
//! AppContainer, WASI, …), or concrete script runtimes (node/bun/deno). Those
//! belong to layers built on top of this one.
//!
//! ## Model
//!
//! * A [`CapabilityRules<P>`] is a transparent, **ordered array** of
//!   [`Capability<P>`] entries. Cascading across configuration levels
//!   (workspace → project → unit → action) is plain concatenation in
//!   declaration order via [`merge::Merge`].
//! * Each entry is an allow/deny [`CapabilityRule`] over an abstract
//!   [`CapabilityDomain`] (`fs.read`, `fs.write`, `net`, `env`, `process`),
//!   tagged with an [`AppliesTo<P>`] selector whose shared base carries the
//!   [`Subsystem`] and whose extras are chosen by the profile.
//! * A [`CapabilityProfile`] (mirroring `omni_input_schema::InputProfile`)
//!   specializes the model per subsystem: `SUPPORTED` domains, `AppliesToExtra`,
//!   per-entry `Extra`, the evaluation `Context`, and the `merge_entry` /
//!   `applies` / `decide` behavior hooks.
//!
//! ## Decisions
//!
//! [`evaluate`] filters the chain to the entries that apply in the current
//! [`CapabilityContext`] and asks the profile to [`decide`](CapabilityProfile::decide).
//! The default strategy ([`deny_dominates`]) is **fail-closed** and
//! **deny-dominant**: unmatched requests are denied, and any matching `deny`
//! wins regardless of position, so a more-specific level can only ever *narrow*
//! authority. [`DenyReason`] carries the "show why" detail.
//!
//! ## Enforcement boundary
//!
//! [`project`] lowers a policy into a neutral [`RequiredCapabilities`]
//! description. Whether a platform can actually *enforce* that description — and
//! what to do when it cannot — is decided by enforcement backends in a separate
//! layer, never here.
//!
//! The [`base`] `()` profile is the neutral default; subsystem profiles (such
//! as the generator profile in `omni_generator_configurations`) implement
//! [`CapabilityProfile`] on their own marker types.

pub mod base;
pub mod error;
pub mod eval;
pub mod json_schema;
pub mod matching;
pub mod model;
pub mod policy_config;
pub mod profile;

// @anchor:mods

pub use error::{Error, ErrorKind};
pub use eval::{
    CapabilityAtom, CapabilityId, Decision, DenyCause, DenyReason, DomainRules,
    RequiredCapabilities, deny_dominates, evaluate, evaluate_layered,
    last_match_wins, project, validate,
};
pub use matching::{PathRoots, Request, rule_matches};
// Re-exported so callers can name the default root vocabulary that `PathRoots`
// is generic over, without depending on `omni_types` directly.
pub use model::{
    Access, Capability, CapabilityDomain, CapabilityRule, CapabilityRules,
    UnenforceablePolicy,
};
pub use omni_types::{OmniPathRoot, Root};
pub use policy_config::{CapabilitiesStrictness, CapabilityPolicyConfig};
pub use profile::{CapabilityProfile, NoExtra};

// @anchor:uses
