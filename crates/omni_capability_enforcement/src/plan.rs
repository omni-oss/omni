//! Composing backends into an [`EnforcementPlan`], with two independent
//! fail-closed guards:
//!
//! 1. **Coverage** ([`require_full_coverage`]): every domain the policy locks
//!    down must be enforceable by *some* selected backend, or the run is
//!    refused. This is a coarse, domain-level check.
//! 2. **Representability**: within a covered domain, an individual pattern may
//!    still be beyond a backend's mechanism (a [`Gap`]). Gaps are resolved
//!    best-effort — a backend that [enforces the domain
//!    exactly](EnforcementBackend::enforces_exactly) (the in-process broker)
//!    covers them, and even OS-level backends may represent what a pre-spawn
//!    backend cannot. Only a *genuinely* unenforceable pattern — one no backend
//!    can represent — is subject to the configurable
//!    effective [`UnenforceablePolicy`], which **defaults to `deny`**.
//!
//! Separately from those two guards, [`build_plan`] also reports **floor gaps**
//! ([`FloorGap`]): domains the policy governs that *are* covered, but only by a
//! bypassable in-process mechanism ([`Tier::InProcessBroker`]) with no
//! un-bypassable [`Tier::PreSpawnFlags`] / [`Tier::OsSandbox`] floor. These are
//! diagnostics, never fatal here — the broker/shim still run as defense in
//! depth — but they mark where enforcement rests on a mechanism a hostile
//! script could circumvent (e.g. `net`/`process` on Bun, or `fs` off-Linux
//! where there is no OS sandbox yet).

use std::collections::{BTreeMap, BTreeSet};

use omni_capabilities::{
    CapabilityDomain, CapabilityId, RequiredCapabilities, UnenforceablePolicy,
};

use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError, Gap,
    PatternResolver, ShimLayer, ShimPolicy, SpawnPolicy, Tier,
};

/// The fully-composed, ready-to-apply result of enforcing a policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnforcementPlan {
    /// The merged pre-spawn launch restrictions (e.g. Deno flags).
    pub spawn: SpawnPolicy,
    /// The residual policy a script-level shim must enforce in-process, for the
    /// domains (`net`/`process`) the runtime's launch flags could not confine
    /// precisely. Empty when the flags cover everything (or no shim is in the
    /// stack). See [`ShimPolicy`].
    pub shim: ShimPolicy,
    /// Patterns that will not be enforced, surfaced for every gap whose
    /// effective [`UnenforceablePolicy`] is `warn` (a gap resolved to `deny`
    /// errors instead; one resolved to `allow` is silent).
    pub warnings: Vec<String>,
    /// Domains the policy governs that are enforced **only** by a bypassable
    /// in-process mechanism, with no un-bypassable runtime-flag or OS-sandbox
    /// floor for the resolved runtime/platform. See [`FloorGap`]. Always
    /// surfaced (they are not subject to [`UnenforceablePolicy`]); never fatal
    /// here.
    pub floor_gaps: Vec<FloorGap>,
}

/// A domain the policy **governs** (has an explicit allow/deny rule) that is
/// covered, but only by a **bypassable** in-process mechanism
/// ([`Tier::InProcessBroker`] — the RPC broker or the script shim), with no
/// [`Tier::PreSpawnFlags`] / [`Tier::OsSandbox`] backend providing an
/// un-bypassable floor for it.
///
/// The broker/shim still enforce the domain for well-behaved I/O, so this is
/// defense-in-depth, not containment: a script reaching the resource *directly*
/// (raw sockets, direct filesystem syscalls, FFI/N-API/WASM, a self-crafted
/// module binding) escapes it. Typical cases: `net`/`process` on Bun (no
/// permission model) and any `fs` domain off-Linux (no OS sandbox integrated on
/// macOS/Windows yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorGap {
    /// The restricted domain lacking an un-bypassable floor.
    pub domain: CapabilityDomain,
    /// Human-readable explanation naming the bypassable mechanism(s) that do
    /// cover it, for "show why" diagnostics.
    pub reason: String,
}

/// How [`build_plan_strict`] treats [`FloorGap`]s — governed domains that rest
/// only on a bypassable [`Tier::InProcessBroker`] with no un-bypassable
/// [`Tier::PreSpawnFlags`] / [`Tier::OsSandbox`] floor for the resolved
/// runtime/platform.
///
/// The default is [`Warn`](FloorStrictness::Warn): floor gaps are surfaced as
/// diagnostics on [`EnforcementPlan::floor_gaps`] but never fatal (the
/// broker/shim still run as defense in depth). A caller that wants a stronger
/// stance can opt into [`RequireFloor`](FloorStrictness::RequireFloor), which
/// turns any floor gap into a fail-closed [`EnforcementError::no_floor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FloorStrictness {
    /// Surface floor gaps as diagnostics and proceed (the shipped default).
    #[default]
    Warn,
    /// Refuse to build a plan if any governed domain lacks an un-bypassable
    /// floor for the resolved runtime/platform.
    RequireFloor,
}

fn backend_names(backends: &[&dyn EnforcementBackend]) -> String {
    if backends.is_empty() {
        return "<none>".to_string();
    }
    backends
        .iter()
        .map(|b| b.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Ensure every domain the policy restricts is covered by at least one backend.
///
/// This is a **domain-level** check: it verifies the platform has *a* mechanism
/// for each resource kind. Whether a specific pattern is representable is
/// handled separately, during [`build_plan`].
pub fn require_full_coverage(
    req: &RequiredCapabilities,
    backends: &[&dyn EnforcementBackend],
) -> Result<(), EnforcementError> {
    for &domain in &req.restricted {
        let covered = backends.iter().any(|b| b.coverage().covers(domain));
        if !covered {
            return Err(EnforcementError::uncovered_domain(
                domain,
                backend_names(backends),
            ));
        }
    }
    Ok(())
}

/// Build a complete [`EnforcementPlan`] using the fail-closed default
/// ([`UnenforceablePolicy::Deny`]) for any gap whose rule did not set its own
/// [`on_unenforceable`](omni_capabilities::CapabilityRule::on_unenforceable).
pub fn build_plan(
    req: &RequiredCapabilities,
    roots: &dyn PatternResolver,
    backends: &[&dyn EnforcementBackend],
) -> Result<EnforcementPlan, EnforcementError> {
    build_plan_with(req, roots, backends, UnenforceablePolicy::default())
}

/// Build a complete [`EnforcementPlan`], deciding each genuinely-unenforceable
/// pattern per its effective [`UnenforceablePolicy`], with the default
/// [`FloorStrictness::Warn`] stance (floor gaps are diagnostics, never fatal).
///
/// The effective policy for a gap is the explicit per-rule choice recorded in
/// [`RequiredCapabilities::unenforceable`] if present, otherwise `default`. This
/// is what makes the decision **fine-grained**: a critical `deny **/.git/**` can
/// keep failing closed while a best-effort `net *.cdn.example` degrades to a
/// warning, in the same policy.
///
/// Steps: verify domain coverage → collect each backend's best-effort
/// contribution and gaps → resolve gaps against the stack → apply each
/// remaining gap's effective policy. If *any* gap resolves to `deny`, the run is
/// refused (deny dominates); the rest are surfaced as warnings (`warn`) or
/// dropped (`allow`).
///
/// To additionally refuse when a governed domain has no un-bypassable floor, use
/// [`build_plan_strict`] with [`FloorStrictness::RequireFloor`].
pub fn build_plan_with(
    req: &RequiredCapabilities,
    roots: &dyn PatternResolver,
    backends: &[&dyn EnforcementBackend],
    default: UnenforceablePolicy,
) -> Result<EnforcementPlan, EnforcementError> {
    build_plan_strict(req, roots, backends, default, FloorStrictness::Warn)
}

/// Build a complete [`EnforcementPlan`] with an explicit [`FloorStrictness`].
///
/// Identical to [`build_plan_with`] except for how it treats **floor gaps**
/// (governed domains covered only by a bypassable in-process mechanism, with no
/// un-bypassable runtime-flag or OS-sandbox floor):
///
/// * [`FloorStrictness::Warn`] (the default) — they are recorded on
///   [`EnforcementPlan::floor_gaps`] for the caller to surface, and the plan
///   still builds. This is the shipped interim behaviour.
/// * [`FloorStrictness::RequireFloor`] — any floor gap fails the plan with
///   [`EnforcementError::no_floor`], so a run only proceeds when every governed
///   domain has an un-bypassable floor for the resolved runtime/platform.
///
/// The floor decision runs *after* coverage and representability, so a
/// `RequireFloor` refusal specifically means "covered, but only bypassably" —
/// distinct from an uncovered domain or an unenforceable pattern.
pub fn build_plan_strict(
    req: &RequiredCapabilities,
    roots: &dyn PatternResolver,
    backends: &[&dyn EnforcementBackend],
    default: UnenforceablePolicy,
    strictness: FloorStrictness,
) -> Result<EnforcementPlan, EnforcementError> {
    // A single-level plan: the merged requirements *are* the only level, so the
    // shim residual is a one-layer fold (identical to the old flat behaviour).
    build_plan_layered(
        req,
        std::slice::from_ref(req),
        roots,
        backends,
        default,
        strictness,
    )
}

/// Build a complete [`EnforcementPlan`] from an ordered stack of **policy
/// levels**, so the script-level shim residual can be folded by the same
/// shrink-only (attenuation) model the Rust broker applies to `fs`.
///
/// * `req` is the *merged* (union) projection of every level. It drives coverage,
///   the pre-spawn launch flags, and the floor-gap analysis — all of which only
///   need a conservative **superset** (a coarse flag must never block an
///   operation an inner level allows).
/// * `levels` are the per-level projections, ordered outermost → innermost
///   (workspace floor, ancestor generators, this generator, this action). Only
///   the layered [`ShimPolicy`] residual consumes them; it hands each level's
///   `net`/`process` rules to the shim as a distinct layer so a deeper level can
///   only ever narrow — never widen — an ancestor's allow-list.
///
/// Passing `std::slice::from_ref(req)` reduces this to the single-level
/// [`build_plan_strict`] behaviour.
pub fn build_plan_layered(
    req: &RequiredCapabilities,
    levels: &[RequiredCapabilities],
    roots: &dyn PatternResolver,
    backends: &[&dyn EnforcementBackend],
    default: UnenforceablePolicy,
    strictness: FloorStrictness,
) -> Result<EnforcementPlan, EnforcementError> {
    require_full_coverage(req, backends)?;

    let mut spawn = SpawnPolicy::new();
    let mut all_gaps: Vec<Gap> = Vec::new();
    for backend in backends {
        let BackendPlan {
            spawn: contribution,
            gaps,
        } = backend.plan(req, roots)?;
        spawn.extend(contribution);
        all_gaps.extend(gaps);
    }

    // Which domains had at least one pre-spawn representability gap, before any
    // shim resolves them. This drives the shim residual: a shim domain with a
    // gap (or covered by no pre-spawn backend at all) could not be confined
    // precisely at launch, so its rules are handed to the shim.
    let domains_with_gaps: BTreeSet<CapabilityDomain> =
        all_gaps.iter().map(|g| g.domain).collect();
    let shim = compute_shim_policy(req, levels, backends, &domains_with_gaps);

    let genuine = genuine_gaps(all_gaps, backends);

    // The id→stance lookup is built once from the merged requirement; a genuine
    // gap resolves its `on_unenforceable` by echoed id, never by re-deriving a
    // pattern string.
    let unenforceable = collect_unenforceable(req);

    let mut deny_gaps: Vec<Gap> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for gap in genuine {
        match effective_policy(&unenforceable, &gap, default) {
            UnenforceablePolicy::Deny => deny_gaps.push(gap),
            UnenforceablePolicy::Warn => warnings.push(gap_line(&gap)),
            UnenforceablePolicy::Allow => {}
        }
    }

    if !deny_gaps.is_empty() {
        return Err(EnforcementError::unenforceable(render_gaps(&deny_gaps)));
    }

    let floor_gaps = floor_gaps(req, backends);
    if strictness == FloorStrictness::RequireFloor && !floor_gaps.is_empty() {
        return Err(EnforcementError::no_floor(render_floor_gaps(&floor_gaps)));
    }

    Ok(EnforcementPlan {
        spawn,
        shim,
        warnings,
        floor_gaps,
    })
}

/// Identify domains the policy **governs** (has an explicit allow/deny rule)
/// that have **no un-bypassable floor**: no [`Tier::PreSpawnFlags`] or
/// [`Tier::OsSandbox`] backend in the stack covers them, so the only thing
/// enforcing the domain is a bypassable [`Tier::InProcessBroker`] (the RPC
/// broker or the script shim).
///
/// This is reported for diagnostics, not as a hard failure: the broker/shim are
/// still active as defense in depth. It is deliberately domain-level and
/// mechanism-derived, so it stays correct as backends are added — e.g. once a
/// Seatbelt/AppContainer [`Tier::OsSandbox`] backend covers `fs` off-Linux, the
/// corresponding floor gaps disappear on their own.
///
/// Every governed domain is already known to be *covered* (coverage was checked
/// first), so the covering mechanism is always some in-process backend.
fn floor_gaps(
    req: &RequiredCapabilities,
    backends: &[&dyn EnforcementBackend],
) -> Vec<FloorGap> {
    let mut gaps = Vec::new();
    for &domain in &req.restricted {
        // Only domains the policy actively *governs* (an explicit allow/deny
        // rule) are actionable here. Every domain is `restricted` under
        // default-deny, but a domain the generator never references is denied
        // wholesale and not something the author asked to have precisely
        // enforced — warning about it on every run would be noise. (Making that
        // whole-domain deny un-bypassable on a floor-less runtime is the
        // separate "require an OS sandbox" gate, deferred to the OS-sandbox
        // backends.)
        let governed = req
            .domains
            .get(&domain)
            .is_some_and(|r| !r.allow.is_empty() || !r.deny.is_empty());
        if !governed {
            continue;
        }
        let floored = backends
            .iter()
            .any(|b| b.tier().provides_floor() && b.coverage().covers(domain));
        if !floored {
            gaps.push(FloorGap {
                domain,
                reason: floor_gap_reason(domain, backends),
            });
        }
    }
    gaps
}

/// Explain a [`FloorGap`] by naming the bypassable in-process mechanism(s) that
/// do cover the domain (via ordinary coverage or as a script shim).
fn floor_gap_reason(
    domain: CapabilityDomain,
    backends: &[&dyn EnforcementBackend],
) -> String {
    let mut mechanisms: Vec<&str> = backends
        .iter()
        .filter(|b| !b.tier().provides_floor())
        .filter(|b| {
            b.coverage().covers(domain) || b.shim_domains().covers(domain)
        })
        .map(|b| b.name())
        .collect();
    mechanisms.dedup();

    if mechanisms.is_empty() {
        // Defensive: coverage was already checked, so this should not occur.
        return format!(
            "{domain} has no un-bypassable runtime-flag or OS-sandbox floor for \
             this runtime/platform"
        );
    }
    format!(
        "{domain} is enforced only by the bypassable in-process mechanism(s) \
         ({}) with no runtime-flag or OS-sandbox floor for this \
         runtime/platform; a script reaching the resource directly (raw \
         sockets, direct syscalls, or FFI/N-API/WASM) can bypass it",
        mechanisms.join(", ")
    )
}

/// Compute the layered [`ShimPolicy`] residual: for every domain a script shim
/// in the stack is responsible for, decide whether the runtime's launch flags
/// already confine it *precisely*. If they do, the shim skips it (absent from
/// [`ShimPolicy::enforced`]); otherwise the domain is marked enforced and each
/// policy level's precise rules are handed to the shim as a distinct
/// [`ShimLayer`], so the shim can fold them by attenuation (a deeper level can
/// only narrow an ancestor's allow-list, never widen it).
///
/// "Precisely confined by flags" means some [`Tier::PreSpawnFlags`] backend both
/// covers the domain and reported no gap for it (so its emitted flags fully
/// express the policy — including a deny-all by omission) **and** at most one
/// level constrains the domain (so the union the flags are lowered from equals
/// the exact effective authority). A domain with a gap, one no pre-spawn backend
/// covers (e.g. Bun's `net`), or one constrained by two or more levels (where
/// the flag-level union is a superset of the true intersection) is left to the
/// shim.
///
/// The shim-responsibility and precise-flag decisions are made from the *merged*
/// `req` (they are per-domain, level-independent); only the per-layer rules come
/// from `levels`. A level that says nothing about an enforced domain omits it
/// from its layer (pass-through for that level); a domain enforced but granted
/// by no layer is deny-all by the fold's fail-closed rule.
fn compute_shim_policy(
    req: &RequiredCapabilities,
    levels: &[RequiredCapabilities],
    backends: &[&dyn EnforcementBackend],
    domains_with_gaps: &BTreeSet<CapabilityDomain>,
) -> ShimPolicy {
    let shim_domains: Coverage = Coverage::of(
        backends
            .iter()
            .flat_map(|b| b.shim_domains().domains().collect::<Vec<_>>()),
    );

    // Which shim domains the shim is actually responsible for (level-independent).
    let mut enforced: BTreeSet<CapabilityDomain> = BTreeSet::new();
    for domain in shim_domains.domains() {
        // Not restricted → nothing to enforce anywhere.
        if !req.restricted.contains(&domain) {
            continue;
        }
        let precisely_flagged = backends.iter().any(|b| {
            b.tier() == Tier::PreSpawnFlags
                && b.coverage().covers(domain)
                && !domains_with_gaps.contains(&domain)
        });
        // How many levels actually constrain this domain. The pre-spawn flags
        // are lowered from the *merged* (union) requirements, so they enforce
        // the effective policy **exactly** only when a single level constrains
        // the domain. With two or more constraining levels the union is a strict
        // superset of the true (intersection) authority — e.g. a child that
        // widened `net` past its workspace ceiling — so the shim must fold the
        // layers and narrow it, even where the flags are otherwise "precise".
        let constraining = levels
            .iter()
            .filter(|l| l.domains.contains_key(&domain))
            .count();
        if precisely_flagged && constraining <= 1 {
            continue;
        }
        enforced.insert(domain);
    }

    // Nothing to enforce → an empty (pure-passthrough) residual.
    if enforced.is_empty() {
        return ShimPolicy::new();
    }

    // One layer per policy level, carrying only the enforced domains that level
    // actually constrains (an unconstrained domain is omitted → pass-through for
    // that level, mirroring the broker's `Permit`).
    let layers: Vec<ShimLayer> = levels
        .iter()
        .map(|level| {
            let mut layer = ShimLayer::default();
            for &domain in &enforced {
                if let Some(rules) = level.domains.get(&domain) {
                    layer.domains.insert(domain, rules.clone().into());
                }
            }
            layer
        })
        // Drop fully-empty layers: a level that constrains none of the enforced
        // domains folds to pass-through, so carrying it changes nothing and only
        // bloats the wire artifact.
        .filter(|layer| !layer.is_empty())
        .collect();

    ShimPolicy { enforced, layers }
}

/// Build the `id → on_unenforceable` lookup from the merged requirement. Only
/// atoms carrying an explicit stance appear; every other id defers to the
/// caller's default. This folds what used to be a separate `(domain, pattern)`
/// side-map onto the atom's opaque id, so a gap's lookup can never miss because
/// a backend rephrased the pattern.
fn collect_unenforceable(
    req: &RequiredCapabilities,
) -> BTreeMap<CapabilityId, UnenforceablePolicy> {
    let mut map = BTreeMap::new();
    for rules in req.domains.values() {
        for atom in rules.allow.iter().chain(rules.deny.iter()) {
            if let Some(policy) = atom.on_unenforceable {
                map.insert(atom.id, policy);
            }
        }
    }
    map
}

/// The effective [`UnenforceablePolicy`] for `gap`: the explicit per-atom choice
/// recorded for its [`CapabilityId`], else the caller's `default`.
fn effective_policy(
    unenforceable: &BTreeMap<CapabilityId, UnenforceablePolicy>,
    gap: &Gap,
    default: UnenforceablePolicy,
) -> UnenforceablePolicy {
    unenforceable.get(&gap.id).copied().unwrap_or(default)
}

/// Reduce raw per-backend gaps to the *genuinely* unenforceable ones: an atom
/// is genuine only if **every backend that covers its domain** reported it as a
/// gap **and** no backend enforces that domain exactly.
///
/// In other words, a gap is resolved if a broker enforces the domain exactly,
/// or if some other covering backend managed to represent that same atom.
///
/// Correlation is keyed on the atom's opaque [`CapabilityId`], which backends
/// echo verbatim — so it stays correct even if a backend normalizes, resolves,
/// or splits the pattern before reporting.
fn genuine_gaps(
    all_gaps: Vec<Gap>,
    backends: &[&dyn EnforcementBackend],
) -> Vec<Gap> {
    // Per-domain: how many backends cover it, and is any an exact enforcer?
    let mut covering: BTreeMap<CapabilityDomain, usize> = BTreeMap::new();
    let mut exact: BTreeMap<CapabilityDomain, bool> = BTreeMap::new();
    for &domain in CapabilityDomain::ALL {
        let cover = backends.iter().filter(|b| b.coverage().covers(domain));
        covering.insert(domain, cover.clone().count());
        exact.insert(domain, cover.clone().any(|b| b.enforces_exactly()));
    }

    // Count distinct atom (by id) gap reports across covering backends, keeping
    // one representative gap per id.
    let mut reports: BTreeMap<CapabilityId, (usize, Gap)> = BTreeMap::new();
    for gap in all_gaps {
        reports
            .entry(gap.id)
            .and_modify(|(n, _)| *n += 1)
            .or_insert((1, gap));
    }

    reports
        .into_values()
        .filter_map(|(count, gap)| {
            let exact_enforced = *exact.get(&gap.domain).unwrap_or(&false);
            let covering_count = *covering.get(&gap.domain).unwrap_or(&0);
            // Genuine iff no exact enforcer and *all* covering backends gapped
            // it (none represented it).
            let genuine = !exact_enforced && count >= covering_count;
            genuine.then_some(gap)
        })
        .collect()
}

fn gap_line(gap: &Gap) -> String {
    format!(
        "{} `{}` ({}): {}",
        gap.domain, gap.pattern, gap.backend, gap.reason
    )
}

fn render_gaps(gaps: &[Gap]) -> String {
    gaps.iter()
        .map(|g| format!("  - {}", gap_line(g)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render [`FloorGap`]s for a [`FloorStrictness::RequireFloor`] refusal.
fn render_floor_gaps(gaps: &[FloorGap]) -> String {
    gaps.iter()
        .map(|g| format!("  - {}", g.reason))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

    use super::*;
    use crate::{
        Coverage, DenoFlags, NoSandbox, NodePermissions, ScriptShimBroker, Tier,
    };

    fn roots() -> PathRoots {
        PathRoots::new().with(Root::Workspace, "/repo")
    }

    fn require(json: &str) -> RequiredCapabilities {
        let cfg: CapabilityRules = serde_json::from_str(json).unwrap();
        project(&cfg, &())
    }

    /// The shim rules a single-level plan hands to the shim for `domain` (the
    /// domain appears in at most one layer for a one-level plan).
    fn shim_rules(
        plan: &EnforcementPlan,
        domain: CapabilityDomain,
    ) -> Option<&crate::ShimDomain> {
        plan.shim.layers.iter().find_map(|l| l.domains.get(&domain))
    }

    #[test]
    fn no_sandbox_alone_fails_closed_for_every_domain() {
        let req = require("[]");
        let backends: [&dyn EnforcementBackend; 1] = [&NoSandbox];
        let err = require_full_coverage(&req, &backends)
            .expect_err("no coverage must fail closed");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::UncoveredDomain);
    }

    #[test]
    fn deno_covers_everything() {
        let req = require("[]");
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        assert!(require_full_coverage(&req, &backends).is_ok());
    }

    #[test]
    fn build_plan_merges_backend_args() {
        let req = require(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] = [&NoSandbox, &DenoFlags];
        let plan = build_plan(&req, &roots(), &backends).expect("covered");
        assert!(plan.spawn.args.contains(&"--allow-read=/repo".to_string()));
    }

    // ── unrepresentable-pattern policy ───────────────────────────────────────

    fn deny_glob_policy() -> RequiredCapabilities {
        // Deno cannot express `deny **/.git/**` as a path prefix → a gap.
        require(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] }
            ]"#,
        )
    }

    #[test]
    fn deny_is_the_default_for_genuine_gaps() {
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let err = build_plan(&deny_glob_policy(), &roots(), &backends)
            .expect_err("default must fail closed");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::Unenforceable);
        // The message names the offending domain/pattern.
        assert!(err.to_string().contains(".git"), "{err}");
    }

    #[test]
    fn warn_proceeds_with_a_warning() {
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let plan = build_plan_with(
            &deny_glob_policy(),
            &roots(),
            &backends,
            UnenforceablePolicy::Warn,
        )
        .expect("warn proceeds");
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains(".git"));
        // The representable allow was still emitted.
        assert!(plan.spawn.args.contains(&"--allow-write=/repo".to_string()));
    }

    #[test]
    fn allow_proceeds_silently() {
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let plan = build_plan_with(
            &deny_glob_policy(),
            &roots(),
            &backends,
            UnenforceablePolicy::Allow,
        )
        .expect("allow proceeds");
        assert!(plan.warnings.is_empty());
    }

    // ── per-rule `on_unenforceable` overrides ────────────────────────────────

    #[test]
    fn per_rule_warn_overrides_the_deny_default() {
        // The `deny **/.git/**` rule opts into `warn`, so even under the
        // fail-closed `build_plan` default the run proceeds with a warning.
        let req = require(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"], "on_unenforceable": "warn" }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("per-rule warn proceeds under the deny default");
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains(".git"));
    }

    #[test]
    fn per_rule_allow_overrides_the_deny_default() {
        let req = require(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"], "on_unenforceable": "allow" }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("per-rule allow proceeds silently under the deny default");
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn a_single_deny_gap_dominates_a_warn_gap() {
        // One gap opts into `warn`, another keeps the `deny` default → the run is
        // still refused (deny dominates), and only the deny gap is reported.
        let req = require(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["*.cdn.example:443"], "on_unenforceable": "warn" },
                { "access": "allow", "domain": "net", "patterns": ["*.secret.example:443"] }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let err = build_plan(&req, &roots(), &backends)
            .expect_err("a deny-defaulted gap must fail the whole plan");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::Unenforceable);
        let msg = err.to_string();
        assert!(msg.contains("secret.example"), "{msg}");
        assert!(
            !msg.contains("cdn.example"),
            "warn gap must not error: {msg}"
        );
    }

    #[test]
    fn most_severe_choice_wins_for_a_shared_pattern() {
        // The same pattern is governed by a `warn` rule and a `deny` rule; the
        // more severe `deny` is kept, so the plan fails closed.
        let req = require(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["*.cdn.example:443"], "on_unenforceable": "warn" },
                { "access": "allow", "domain": "net", "patterns": ["*.cdn.example:443"], "on_unenforceable": "deny" }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 1] = [&DenoFlags];
        let err = build_plan(&req, &roots(), &backends)
            .expect_err("deny is more severe than warn for the shared pattern");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::Unenforceable);
    }

    // ── opaque-id correlation / lookup (verbatim-echo invariant removed) ──────

    /// A pre-spawn backend that covers `net` but deliberately **rewrites** the
    /// pattern before reporting a [`Gap`], while echoing the atom's opaque
    /// [`CapabilityId`]. It stands in for a future OS-sandbox backend that
    /// lowers patterns into a *different* vocabulary (SBPL filters, AppContainer
    /// capability SIDs), and exists to prove the planner correlates gaps and
    /// resolves `on_unenforceable` by id rather than by the verbatim source
    /// string.
    struct RewritingNetBackend {
        label: &'static str,
        prefix: &'static str,
    }
    impl EnforcementBackend for RewritingNetBackend {
        fn name(&self) -> &'static str {
            self.label
        }
        fn tier(&self) -> Tier {
            Tier::PreSpawnFlags
        }
        fn coverage(&self) -> Coverage {
            // Covers everything so `require_full_coverage` passes; only `net`
            // is ever gapped below.
            Coverage::all()
        }
        fn plan(
            &self,
            req: &RequiredCapabilities,
            _roots: &dyn PatternResolver,
        ) -> Result<BackendPlan, EnforcementError> {
            let mut plan = BackendPlan::new();
            if let Some(rules) = req.domains.get(&CapabilityDomain::Net) {
                for atom in &rules.allow {
                    plan.gaps.push(Gap {
                        backend: self.name().to_string(),
                        domain: CapabilityDomain::Net,
                        id: atom.id,
                        // Deliberately NOT the verbatim source pattern.
                        pattern: format!("{}{}", self.prefix, atom.pattern),
                        reason: "cannot express this host in the sandbox \
                                 vocabulary"
                            .to_string(),
                    });
                }
            }
            Ok(plan)
        }
    }

    #[test]
    fn on_unenforceable_resolves_by_id_even_when_the_backend_rewrites_the_pattern()
     {
        // A single net allow rule opts into `warn`. The only covering backend
        // reports the gap against a REWRITTEN pattern (not the verbatim
        // source), echoing the atom's opaque id. The planner must still resolve
        // the atom's `warn` stance by id — under the old (domain, pattern)
        // side-map the rewritten string would miss and fall through to the
        // fail-closed `deny` default, refusing the run.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"], "on_unenforceable": "warn" }]"#,
        );
        let backend = RewritingNetBackend {
            label: "rewriting-net",
            prefix: "lowered::",
        };
        let backends: [&dyn EnforcementBackend; 1] = [&backend];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("the warn stance must be found by id despite the rewrite");
        assert_eq!(plan.warnings.len(), 1);
        // The diagnostic carries the backend's own (rewritten) string, proving
        // the pattern is retained purely for display while the id drives
        // resolution.
        assert!(
            plan.warnings[0].contains("lowered::example.com:443"),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn a_gap_is_correlated_by_id_across_backends_that_rewrite_differently() {
        // Two covering backends both gap the SAME atom, but each lowers the
        // pattern into its OWN vocabulary (different strings). Correlation by id
        // sees one atom gapped by every covering backend → genuine → the
        // default `deny` refuses the run. Under (domain, pattern) keying the two
        // differing strings would look like two separate half-covered gaps and
        // neither would reach `count >= covering_count`, silently dropping the
        // confinement.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let a = RewritingNetBackend {
            label: "sandbox-a",
            prefix: "a::",
        };
        let b = RewritingNetBackend {
            label: "sandbox-b",
            prefix: "b::",
        };
        let backends: [&dyn EnforcementBackend; 2] = [&a, &b];
        let err = build_plan(&req, &roots(), &backends)
            .expect_err("the id-correlated genuine gap must fail closed");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::Unenforceable);
    }

    /// A mock in-process broker that enforces every covered domain exactly.
    struct MockBroker;
    impl EnforcementBackend for MockBroker {
        fn name(&self) -> &'static str {
            "mock-broker"
        }
        fn tier(&self) -> Tier {
            Tier::InProcessBroker
        }
        fn coverage(&self) -> Coverage {
            Coverage::all()
        }
        fn enforces_exactly(&self) -> bool {
            true
        }
        fn plan(
            &self,
            _req: &RequiredCapabilities,
            _roots: &dyn PatternResolver,
        ) -> Result<BackendPlan, EnforcementError> {
            // A broker enforces at runtime; it contributes no spawn flags and,
            // crucially, reports no gaps.
            Ok(BackendPlan::new())
        }
    }

    #[test]
    fn an_exact_broker_resolves_flag_backend_gaps() {
        // Deno gaps `deny **/.git/**`, but the broker enforces fs.write exactly,
        // so the stack has no genuine gap — even under the default `deny`.
        let backends: [&dyn EnforcementBackend; 2] = [&DenoFlags, &MockBroker];
        let plan = build_plan(&deny_glob_policy(), &roots(), &backends)
            .expect("broker resolves the gap");
        // Deno still contributes its coarse allow.
        assert!(plan.spawn.args.contains(&"--allow-write=/repo".to_string()));
        assert!(plan.warnings.is_empty());
    }

    // ── script-shim residual (net/process) ───────────────────────────────────

    #[test]
    fn deno_precise_net_needs_no_shim_residual() {
        // Deno expresses `example.com:443` precisely, so the shim skips `net`
        // entirely and Deno emits the precise flag (not the broad floor).
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &ScriptShimBroker::new()];
        let plan = build_plan(&req, &roots(), &backends).expect("precise");
        assert!(
            plan.spawn
                .args
                .contains(&"--allow-net=example.com:443".to_string()),
            "{:?}",
            plan.spawn.args
        );
        assert!(!plan.spawn.args.contains(&"--allow-net".to_string()));
        assert!(
            plan.shim.is_empty(),
            "precise net must not be handed to the shim: {:?}",
            plan.shim
        );
    }

    #[test]
    fn deno_wildcard_net_falls_back_to_floor_plus_shim() {
        // Deno cannot express a host wildcard, so it grants the broad floor and
        // the precise rule is handed to the shim.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["*.cdn.example:443"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &ScriptShimBroker::new()];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("shim resolves the wildcard gap");
        // Broad floor, not a precise value.
        assert!(plan.spawn.args.contains(&"--allow-net".to_string()));
        assert!(plan.shim.enforced.contains(&CapabilityDomain::Net));
        let net = shim_rules(&plan, CapabilityDomain::Net)
            .expect("net handed to shim");
        assert_eq!(net.allow, vec!["*.cdn.example:443".to_string()]);
    }

    #[test]
    fn node_specific_host_uses_broad_floor_and_shim_narrows() {
        // Node's --allow-net is all-or-nothing: broad floor + shim residual.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 3] =
            [&NodePermissions, &FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan(&req, &roots(), &backends)
            .expect("shim resolves node's coarse net");
        assert!(plan.spawn.args.contains(&"--allow-net".to_string()));
        let net = shim_rules(&plan, CapabilityDomain::Net).unwrap();
        assert_eq!(net.allow, vec!["example.com:443".to_string()]);
    }

    #[test]
    fn bun_like_stack_hands_whole_domain_to_shim() {
        // No pre-spawn flag backend covers net/process (Bun): coverage passes
        // because the shim covers them, and the whole policy is the residual.
        let req = require(
            r#"[
                { "access": "allow", "domain": "net",     "patterns": ["example.com:443"] },
                { "access": "allow", "domain": "process", "patterns": ["git"] }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 1] = [&ScriptShimBroker::new()];
        let plan = build_plan(&req, &roots(), &backends)
            .expect_err("fs is uncovered by a shim-only stack");
        // Sanity: the shim covers net/process/env but not fs, so the stack still
        // fails closed on the uncovered filesystem domains — proving the shim is
        // not a blanket bypass.
        assert_eq!(plan.kind(), crate::EnforcementErrorKind::UncoveredDomain);
    }

    #[test]
    fn shim_covers_net_process_and_env_together_with_fs_backends() {
        // A realistic Bun-style stack: OS sandbox + RPC broker cover fs/env,
        // the shim owns net/process precisely and additionally marks `env`
        // (which has no launch-flag/OS floor on Bun) so the layered env rules
        // reach the JS side. The net/process policy is the residual; env is
        // enforced too, here as a deny-all (the policy grants it no rule).
        let req = require(
            r#"[
                { "access": "allow", "domain": "net",     "patterns": ["example.com:443"] },
                { "access": "allow", "domain": "process", "patterns": ["git"] }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan =
            build_plan(&req, &roots(), &backends).expect("fully covered");
        assert!(plan.shim.enforced.contains(&CapabilityDomain::Net));
        assert!(plan.shim.enforced.contains(&CapabilityDomain::Process));
        // `env` is a shim domain with no floor on this stack, so it is enforced
        // even though the policy grants it nothing (fail-closed deny-all): no
        // layer carries an env rule.
        assert!(plan.shim.enforced.contains(&CapabilityDomain::Env));
        assert!(
            shim_rules(&plan, CapabilityDomain::Env).is_none(),
            "an ungranted env domain contributes no layer: {:?}",
            plan.shim
        );
        assert_eq!(
            shim_rules(&plan, CapabilityDomain::Net).unwrap().allow,
            vec!["example.com:443".to_string()]
        );
        assert_eq!(
            shim_rules(&plan, CapabilityDomain::Process).unwrap().allow,
            vec!["git".to_string()]
        );
    }

    #[test]
    fn layered_shim_residual_keeps_each_level_distinct() {
        // A Bun-style stack (shim owns net/process). Two policy levels: an outer
        // ceiling and an inner level that *tries* to widen net past it. The
        // residual must carry BOTH levels as distinct layers so the runtime shim
        // can fold them by attenuation (the inner widening is capped there). The
        // merged `req` (the union) is what coverage/flags see.
        let outer = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["*.example.com:443"] }]"#,
        );
        let inner = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["evil.com:443"] }]"#,
        );
        // The merged (union) superset the coarse flags/coverage see.
        let merged = require(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["*.example.com:443"] },
                { "access": "allow", "domain": "net", "patterns": ["evil.com:443"] }
            ]"#,
        );

        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan_layered(
            &merged,
            &[outer, inner],
            &roots(),
            &backends,
            UnenforceablePolicy::default(),
            FloorStrictness::Warn,
        )
        .expect("shim covers net");

        assert!(plan.shim.enforced.contains(&CapabilityDomain::Net));
        assert_eq!(plan.shim.layers.len(), 2, "{:?}", plan.shim);
        assert_eq!(
            plan.shim.layers[0].domains[&CapabilityDomain::Net].allow,
            vec!["*.example.com:443".to_string()]
        );
        assert_eq!(
            plan.shim.layers[1].domains[&CapabilityDomain::Net].allow,
            vec!["evil.com:443".to_string()]
        );
    }

    #[test]
    fn two_levels_keep_net_on_the_shim_even_when_deno_flags_are_precise() {
        // Both levels express host:port Deno can flag precisely, so a
        // single-level plan would skip the shim for `net`. But two levels
        // constrain it, and the Deno flags are lowered from the *union*
        // (a.example + b.example) — a superset of the intersection. The shim
        // must therefore stay responsible so it can narrow the union to the
        // per-level intersection in-process.
        let outer = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["a.example:443"] }]"#,
        );
        let inner = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["b.example:443"] }]"#,
        );
        let merged = require(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["a.example:443"] },
                { "access": "allow", "domain": "net", "patterns": ["b.example:443"] }
            ]"#,
        );

        let backends: [&dyn EnforcementBackend; 3] =
            [&DenoFlags, &FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan_layered(
            &merged,
            &[outer, inner],
            &roots(),
            &backends,
            UnenforceablePolicy::default(),
            FloorStrictness::Warn,
        )
        .expect("deno flags cover the union; shim narrows it");

        assert!(
            plan.shim.enforced.contains(&CapabilityDomain::Net),
            "two constraining levels must keep net on the shim: {:?}",
            plan.shim
        );
        assert_eq!(plan.shim.layers.len(), 2, "{:?}", plan.shim);
    }

    #[test]
    fn one_level_still_lets_deno_flags_own_precise_net() {
        // The single-level path is unchanged: with only one constraining level,
        // Deno's precise flags equal the exact authority, so the shim skips net.
        let req = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["a.example:443"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 2] =
            [&DenoFlags, &ScriptShimBroker::new()];
        let plan = build_plan(&req, &roots(), &backends).expect("precise");
        assert!(!plan.shim.enforced.contains(&CapabilityDomain::Net));
    }

    #[test]
    fn a_level_that_omits_a_shim_domain_contributes_no_layer() {
        // Outer level constrains net; inner level says nothing about net. The
        // inner level folds to pass-through, so it contributes no layer (the
        // empty layer is dropped from the wire artifact).
        let outer = require(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        );
        let inner = require("[]");
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan_layered(
            &outer.clone(),
            &[outer, inner],
            &roots(),
            &backends,
            UnenforceablePolicy::default(),
            FloorStrictness::Warn,
        )
        .expect("shim covers net");
        assert!(plan.shim.enforced.contains(&CapabilityDomain::Net));
        assert_eq!(
            plan.shim.layers.len(),
            1,
            "the silent inner level must not add a layer: {:?}",
            plan.shim
        );
    }

    /// A broker covering fs/env exactly (like the RPC bridge broker) but NOT
    /// net/process — used to stand in for the non-shim half of a Bun stack.
    struct FullBroker;
    impl EnforcementBackend for FullBroker {
        fn name(&self) -> &'static str {
            "full-broker"
        }
        fn tier(&self) -> Tier {
            Tier::InProcessBroker
        }
        fn coverage(&self) -> Coverage {
            Coverage::of([
                CapabilityDomain::FsRead,
                CapabilityDomain::FsWrite,
                CapabilityDomain::Env,
            ])
        }
        fn enforces_exactly(&self) -> bool {
            true
        }
        fn plan(
            &self,
            _req: &RequiredCapabilities,
            _roots: &dyn PatternResolver,
        ) -> Result<BackendPlan, EnforcementError> {
            Ok(BackendPlan::new())
        }
    }

    // ── floor gaps (un-bypassable coverage) ──────────────────────────────────

    /// A stand-in OS-sandbox backend that floors the filesystem domains (like
    /// Landlock) at [`Tier::OsSandbox`], but never net/process/env.
    struct MockOsFsFloor;
    impl EnforcementBackend for MockOsFsFloor {
        fn name(&self) -> &'static str {
            "mock-os-fs"
        }
        fn tier(&self) -> Tier {
            Tier::OsSandbox
        }
        fn coverage(&self) -> Coverage {
            Coverage::of([CapabilityDomain::FsRead, CapabilityDomain::FsWrite])
        }
        fn plan(
            &self,
            _req: &RequiredCapabilities,
            _roots: &dyn PatternResolver,
        ) -> Result<BackendPlan, EnforcementError> {
            Ok(BackendPlan::new())
        }
    }

    fn net_and_process() -> RequiredCapabilities {
        require(
            r#"[
                { "access": "allow", "domain": "net",     "patterns": ["example.com:443"] },
                { "access": "allow", "domain": "process", "patterns": ["git"] }
            ]"#,
        )
    }

    #[test]
    fn prespawn_flags_covering_a_domain_leave_no_floor_gap() {
        // A Deno-style stack: pre-spawn flags cover every domain, so nothing
        // rests on a bypassable mechanism alone.
        let backends: [&dyn EnforcementBackend; 3] =
            [&DenoFlags, &FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan(&net_and_process(), &roots(), &backends)
            .expect("fully covered");
        assert!(
            plan.floor_gaps.is_empty(),
            "pre-spawn flags floor every domain: {:?}",
            plan.floor_gaps
        );
    }

    #[test]
    fn shim_only_domains_are_floor_gaps() {
        // A Bun-style stack with no pre-spawn flags and no OS sandbox: net and
        // process are covered only by the (bypassable) script shim.
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan(&net_and_process(), &roots(), &backends)
            .expect("covered by broker + shim");
        let gapped: Vec<_> = plan.floor_gaps.iter().map(|g| g.domain).collect();
        assert!(gapped.contains(&CapabilityDomain::Net));
        assert!(gapped.contains(&CapabilityDomain::Process));
        // The reason names the covering bypassable mechanism.
        assert!(
            plan.floor_gaps
                .iter()
                .all(|g| g.reason.contains("script-shim")),
            "{:?}",
            plan.floor_gaps
        );
    }

    #[test]
    fn os_sandbox_floors_fs_but_not_net() {
        // A Bun-on-Linux-style stack: the OS sandbox floors fs, but net still
        // rests on the shim alone.
        let req = require(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "net",      "patterns": ["example.com:443"] }
            ]"#,
        );
        let backends: [&dyn EnforcementBackend; 3] =
            [&MockOsFsFloor, &FullBroker, &ScriptShimBroker::new()];
        let plan =
            build_plan(&req, &roots(), &backends).expect("fully covered");
        let gapped: Vec<_> = plan.floor_gaps.iter().map(|g| g.domain).collect();
        assert_eq!(gapped, vec![CapabilityDomain::Net]);
    }

    #[test]
    fn broker_only_env_is_a_floor_gap_even_on_a_flagged_runtime() {
        // Node-style: pre-spawn flags cover fs/net/process, but Node's model does
        // not gate `env`, which then rests on the RPC broker alone.
        let req = require(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["PATH"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 3] =
            [&NodePermissions, &FullBroker, &ScriptShimBroker::new()];
        let plan =
            build_plan(&req, &roots(), &backends).expect("broker covers env");
        let gapped: Vec<_> = plan.floor_gaps.iter().map(|g| g.domain).collect();
        assert_eq!(gapped, vec![CapabilityDomain::Env]);
        assert!(
            plan.floor_gaps[0].reason.contains("full-broker"),
            "{:?}",
            plan.floor_gaps
        );
    }

    #[test]
    fn unrestricted_domains_are_never_floor_gaps() {
        // An empty policy restricts nothing, so there is nothing to floor.
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan(&require("[]"), &roots(), &backends)
            .expect("empty policy");
        assert!(plan.floor_gaps.is_empty());
    }

    // ── floor strictness (require-floor gate) ─────────────────────────────

    #[test]
    fn warn_is_the_default_and_never_refuses_on_a_floor_gap() {
        // Bun-style stack: net/process rest on the shim alone. The default
        // `Warn` stance surfaces them as diagnostics but still builds a plan.
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan(&net_and_process(), &roots(), &backends)
            .expect("warn never refuses");
        assert!(
            !plan.floor_gaps.is_empty(),
            "the floor gaps must still be reported for the caller"
        );
        // `build_plan_with` (also default strictness) agrees.
        assert!(
            build_plan_with(
                &net_and_process(),
                &roots(),
                &backends,
                UnenforceablePolicy::Deny,
            )
            .is_ok()
        );
    }

    #[test]
    fn require_floor_refuses_when_a_governed_domain_has_no_floor() {
        // Same Bun-style stack, but the strict stance turns the net/process
        // floor gaps into a hard, fail-closed refusal.
        let backends: [&dyn EnforcementBackend; 2] =
            [&FullBroker, &ScriptShimBroker::new()];
        let err = build_plan_strict(
            &net_and_process(),
            &roots(),
            &backends,
            UnenforceablePolicy::Deny,
            FloorStrictness::RequireFloor,
        )
        .expect_err("a floor gap must fail closed under require-floor");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::NoFloor);
        // The message names the un-floored domains and the bypassable mechanism.
        let msg = err.to_string();
        assert!(msg.contains("net"), "{msg}");
        assert!(msg.contains("process"), "{msg}");
        assert!(msg.contains("script-shim"), "{msg}");
    }

    #[test]
    fn require_floor_passes_when_every_governed_domain_is_floored() {
        // Deno-style stack: pre-spawn flags floor every domain, so the strict
        // stance is satisfied and builds a plan with no floor gaps.
        let backends: [&dyn EnforcementBackend; 3] =
            [&DenoFlags, &FullBroker, &ScriptShimBroker::new()];
        let plan = build_plan_strict(
            &net_and_process(),
            &roots(),
            &backends,
            UnenforceablePolicy::Deny,
            FloorStrictness::RequireFloor,
        )
        .expect("pre-spawn flags floor every domain");
        assert!(plan.floor_gaps.is_empty());
    }

    #[test]
    fn require_floor_is_satisfied_per_domain_by_an_os_sandbox() {
        // Bun-on-Linux-style: the OS sandbox floors fs, so an fs-only policy
        // passes require-floor even without pre-spawn flags…
        let fs_only = require(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }]"#,
        );
        let backends: [&dyn EnforcementBackend; 3] =
            [&MockOsFsFloor, &FullBroker, &ScriptShimBroker::new()];
        assert!(
            build_plan_strict(
                &fs_only,
                &roots(),
                &backends,
                UnenforceablePolicy::Deny,
                FloorStrictness::RequireFloor,
            )
            .is_ok(),
            "fs is floored by the OS sandbox"
        );

        // …but adding a `net` rule (which the OS sandbox does not floor) makes
        // the same strict stance refuse, and only for `net`.
        let err = build_plan_strict(
            &net_and_process(),
            &roots(),
            &backends,
            UnenforceablePolicy::Deny,
            FloorStrictness::RequireFloor,
        )
        .expect_err("net/process are not OS-sandbox floored");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::NoFloor);
    }

    #[test]
    fn require_floor_still_reports_uncovered_before_no_floor() {
        // Coverage is checked first: a domain no backend covers is an
        // `UncoveredDomain` error, not a `NoFloor` one, even under require-floor.
        // `NoSandbox` covers nothing, so `net` is uncovered.
        let backends: [&dyn EnforcementBackend; 1] = [&NoSandbox];
        let err = build_plan_strict(
            &net_and_process(),
            &roots(),
            &backends,
            UnenforceablePolicy::Deny,
            FloorStrictness::RequireFloor,
        )
        .expect_err("uncovered dominates");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::UncoveredDomain);
    }
}
