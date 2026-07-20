//! The backend abstraction: the contract every enforcement mechanism
//! implements, plus the [`Coverage`] and [`Tier`] vocabulary used to reason
//! about them.
//!
//! The core [`omni_capabilities`] crate answers *"is this operation allowed?"*
//! and lowers a policy into a neutral [`RequiredCapabilities`]. A backend
//! answers the orthogonal question *"can this platform actually **enforce**
//! that, and how?"*. Keeping the two apart is deliberate: policy is pure and
//! portable, enforcement is platform- and runtime-specific.

use std::collections::BTreeSet;

use omni_capabilities::{
    CapabilityDomain, CapabilityId, OmniPathRoot, PathRoots,
    RequiredCapabilities,
};

use crate::{EnforcementError, SpawnPolicy};

/// Where in the defense-in-depth stack a backend operates. Backends from
/// different tiers compose: a policy can be enforced by pre-spawn flags *and*
/// an OS sandbox *and* an in-process broker simultaneously.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Tier {
    /// Restrictions handed to the runtime when it is spawned — e.g. Deno
    /// `--allow-*` / `--deny-*` flags or WASI preopened directories. Cheap,
    /// portable, but only as trustworthy as the runtime honoring them.
    PreSpawnFlags,
    /// A kernel/OS access-control sandbox applied to the process itself —
    /// Landlock (Linux), Seatbelt (macOS), AppContainer (Windows). Strong, but
    /// platform-specific and often coarse-grained (path-prefix / capability
    /// class, not globs). Resource- and lifetime-limiting mechanisms such as
    /// Windows Job Objects are a separate, complementary concern and are not
    /// modelled by this tier.
    OsSandbox,
    /// An in-process broker that authorizes every operation against the policy
    /// at the syscall/RPC boundary (see the generator's `TransactionSys`).
    /// Precise and portable, but only covers I/O routed through omni.
    InProcessBroker,
}

impl Tier {
    /// Whether a backend at this tier provides an **un-bypassable floor**: a
    /// mechanism a confined script cannot circumvent from *inside* its own
    /// runtime.
    ///
    /// [`PreSpawnFlags`](Tier::PreSpawnFlags) qualify because the runtime
    /// enforces them in native code beneath the JS boundary, and
    /// [`OsSandbox`](Tier::OsSandbox) qualifies because the kernel enforces it
    /// on the process itself — neither can be lifted by script code.
    /// [`InProcessBroker`](Tier::InProcessBroker) does **not**: it runs in the
    /// script's own runtime and is bypassable by direct syscalls, raw sockets,
    /// FFI/N-API/WASM, or a self-crafted module binding. A restricted domain
    /// with no floor-tier backend covering it is therefore enforced only as
    /// defense-in-depth (see [`EnforcementPlan::floor_gaps`](crate::EnforcementPlan)).
    pub const fn provides_floor(self) -> bool {
        matches!(self, Tier::PreSpawnFlags | Tier::OsSandbox)
    }
}

/// The set of [`CapabilityDomain`]s a backend can actually enforce **on the
/// platform it is currently running on**.
///
/// Coverage answers *"can this backend restrict this kind of resource at
/// all?"* — not *"can it express this particular pattern?"*. The latter is a
/// per-pattern question answered (and possibly rejected) by
/// [`EnforcementBackend::plan`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage(BTreeSet<CapabilityDomain>);

impl Coverage {
    /// Covers nothing — the honest answer for an unavailable mechanism. Using
    /// only no-coverage backends makes every restricted domain a gap, so the
    /// run fails closed.
    pub fn none() -> Self {
        Self(BTreeSet::new())
    }

    /// Covers every domain the core model knows about.
    pub fn all() -> Self {
        Self(CapabilityDomain::ALL.iter().copied().collect())
    }

    pub fn of(domains: impl IntoIterator<Item = CapabilityDomain>) -> Self {
        Self(domains.into_iter().collect())
    }

    pub fn covers(&self, domain: CapabilityDomain) -> bool {
        self.0.contains(&domain)
    }

    pub fn domains(&self) -> impl Iterator<Item = CapabilityDomain> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Resolves policy patterns into concrete, platform-neutral path globs.
///
/// This is the object-safe slice of [`PathRoots`] that backends need: it lets
/// [`EnforcementBackend::plan`] stay dyn-compatible (so heterogeneous backends
/// can be composed) while still being generic over the caller's root
/// vocabulary. See [`PathRoots::resolve_pattern`].
pub trait PatternResolver {
    /// Resolve an `@root/tail` or plain pattern into a concrete glob, or `None`
    /// if it references an unregistered root (and thus matches nothing).
    fn resolve(&self, pattern: &str) -> Option<String>;
}

impl<R: OmniPathRoot> PatternResolver for PathRoots<R> {
    fn resolve(&self, pattern: &str) -> Option<String> {
        self.resolve_pattern(pattern)
    }
}

/// A single policy pattern a backend **could not faithfully enforce** with its
/// own mechanism (e.g. a mid-path glob for a path-prefix permission model, or a
/// filesystem `deny` for Node which has no deny-list).
///
/// A gap is not automatically fatal: another backend in the stack may enforce
/// the domain exactly (see [`EnforcementBackend::enforces_exactly`]), and even
/// when nothing does, what to do about it is a configurable decision made by
/// the orchestrator rather than the backend.
///
/// A backend reports a gap by **echoing the offending atom's opaque
/// [`CapabilityId`]** (from the [`RequiredCapabilities`] it was handed) rather
/// than re-deriving a string, so the planner correlates gaps across backends and
/// resolves each atom's `on_unenforceable` stance by identity. The verbatim
/// `pattern` is carried alongside purely for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// The backend that could not represent the pattern.
    pub backend: String,
    pub domain: CapabilityDomain,
    /// The opaque surrogate key of the atom this gap is about, echoed verbatim
    /// from the source [`CapabilityAtom`](omni_capabilities::CapabilityAtom).
    /// The planner keys correlation and `on_unenforceable` lookup on this.
    pub id: CapabilityId,
    /// The verbatim source pattern, for "show why" diagnostics only.
    pub pattern: String,
    /// Human-readable explanation, for "show why" diagnostics.
    pub reason: String,
}

/// What a backend produced for a policy: the restrictions it *could* express,
/// plus the patterns it could not (its [`Gap`]s). This is the "best effort"
/// contract — a backend enforces everything it can and reports the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackendPlan {
    pub spawn: SpawnPolicy,
    pub gaps: Vec<Gap>,
}

impl BackendPlan {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A single enforcement mechanism.
///
/// The trait is intentionally **object-safe** so that a full defense-in-depth
/// stack can be held as `&[&dyn EnforcementBackend]` and composed by
/// [`crate::build_plan`].
pub trait EnforcementBackend {
    /// Stable identifier used in diagnostics (e.g. `"deno-flags"`).
    fn name(&self) -> &'static str;

    /// Which tier this backend belongs to.
    fn tier(&self) -> Tier;

    /// Domains this backend can enforce on the current platform. Implementations
    /// should probe the platform (not just the target triple) where relevant.
    fn coverage(&self) -> Coverage;

    /// Whether this backend enforces **every domain it covers exactly** — i.e.
    /// faithfully, for any pattern, with no representability limits. This is
    /// true only for a per-operation broker that evaluates the full policy at
    /// runtime (the [`Tier::InProcessBroker`] tier); a broker in the stack
    /// therefore resolves the [`Gap`]s reported by coarser pre-spawn/OS
    /// backends for the domains it covers. Defaults to `false`.
    fn enforces_exactly(&self) -> bool {
        false
    }

    /// The domains this backend enforces via an **in-runtime script shim** (the
    /// bridge service patching global `fetch` / child-spawn), rather than at the
    /// kernel/RPC boundary. [`crate::build_plan`] uses this to compute the
    /// [`ShimPolicy`](crate::ShimPolicy) residual: for a shim domain the runtime
    /// could not confine precisely, the precise rules are handed to the shim.
    /// Defaults to none (most backends are not script shims).
    fn shim_domains(&self) -> Coverage {
        Coverage::none()
    }

    /// Translate the domains this backend covers into a [`BackendPlan`]:
    /// contribute what it can express, and report each pattern it cannot as a
    /// [`Gap`]. Backends do **not** decide what happens to a gap — that is the
    /// orchestrator's job (see [`crate::UnenforceablePolicy`]).
    ///
    /// The `Err` path is reserved for genuine internal failures, not for
    /// unrepresentable patterns.
    fn plan(
        &self,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError>;
}
