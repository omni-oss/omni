//! The [`CapabilityProfile`] trait — the extension point that lets a subsystem
//! specialize the capability model, mirroring `omni_input_schema::InputProfile`.
//!
//! A profile is a zero-sized marker (`()`, `Generator`, …) that selects:
//!
//! * `SUPPORTED` — which [`CapabilityDomain`]s the subsystem may express.
//! * `NAME` — a human-readable label used in diagnostics.
//! * `AppliesTo` — the *entire* `applies_to` selector for an entry. The core
//!   imposes no shared fields, so any subsystem-specific selector (which
//!   subsystem, action, runtime, …) is defined here, keeping the core clean.
//! * `Extra` — optional per-entry extras.
//! * `Context` — the opaque evaluation context threaded through `applies` /
//!   `decide` (this is where subsystem-only concepts such as a JS runtime live).
//!
//! Behavior hooks (`merge_entry`, `applies`, `decide`) have safe defaults; a
//! profile overrides them to change folding or decision behavior.

use omni_types::OmniPathRoot;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Capability, CapabilityDomain, Decision, PathRoots, Request,
    eval::deny_dominates,
};

/// Marker + behavior selector for a subsystem's capability model.
pub trait CapabilityProfile:
    Default + Clone + core::fmt::Debug + PartialEq + Sized + 'static
{
    /// Domains this subsystem is allowed to express. Consulted by
    /// [`validate`](crate::validate) and (later) the schema projection.
    /// Defaults to every domain.
    const SUPPORTED: &'static [CapabilityDomain] = CapabilityDomain::ALL;

    /// Human-readable name of this profile/subsystem, used in diagnostics.
    const NAME: &'static str = "capabilities";

    /// The complete `applies_to` selector for an entry under this profile.
    type AppliesTo: Serialize
        + DeserializeOwned
        + JsonSchema
        + core::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync
        + 'static;

    /// Optional per-entry extras.
    type Extra: Serialize
        + DeserializeOwned
        + JsonSchema
        + core::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync
        + 'static;

    /// The opaque evaluation context this subsystem threads through
    /// [`applies`](Self::applies) / [`decide`](Self::decide).
    type Context;

    /// Whether `domain` is expressible by this subsystem.
    fn supports(domain: CapabilityDomain) -> bool {
        Self::SUPPORTED.contains(&domain)
    }

    /// Fold an incoming entry into the accumulated, ordered chain during a
    /// cascade merge. Default preserves declaration order by appending.
    fn merge_entry(
        acc: &mut Vec<Capability<Self>>,
        incoming: Capability<Self>,
    ) {
        acc.push(incoming);
    }

    /// Does this entry apply in `ctx`? The default applies to everything;
    /// profiles override this to scope entries (by action, runtime, etc.).
    fn applies(_applies_to: &Self::AppliesTo, _ctx: &Self::Context) -> bool {
        true
    }

    /// Decide a request against the already-filtered, ordered chain.
    ///
    /// The default is fail-closed and deny-dominant: any matching `deny` wins
    /// regardless of position (so a more-specific level can never re-open what a
    /// broader level denied), and if nothing matches the request is denied.
    ///
    /// Generic over the root enum `R` (as [`PathRoots`] is), so overriding this
    /// does not tie a profile to a particular root vocabulary.
    fn decide<R: OmniPathRoot>(
        rules: &[&Capability<Self>],
        req: &Request,
        roots: &PathRoots<R>,
        _ctx: &Self::Context,
    ) -> Decision {
        deny_dominates(rules, req, roots)
    }
}

/// A flattenable empty extras type.
///
/// We deliberately do *not* use `()` for the flattened extra slots: serde's
/// `#[serde(flatten)]` on a unit type fails to deserialize from a map, whereas
/// an empty braced struct flattens cleanly (and ignores unknown keys).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    serde::Deserialize,
    JsonSchema,
)]
pub struct NoExtra {}
