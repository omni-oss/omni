//! Application logic: filtering the cascaded chain by context, deciding a
//! request, validating a config against a profile, and projecting the policy
//! into a neutral [`RequiredCapabilities`] description for enforcement backends.

use std::collections::{BTreeMap, HashMap};

use omni_types::OmniPathRoot;

use crate::{
    Access, Capability, CapabilityDomain, CapabilityProfile, CapabilityRules,
    Error, PathRoots, Request, UnenforceablePolicy, matching::rule_matches,
};

/// The outcome of authorizing a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Decision::Deny(_))
    }
}

/// Why a request was denied — carries enough to render a "show why" message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyReason {
    pub domain: CapabilityDomain,
    pub value: String,
    pub cause: DenyCause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyCause {
    /// Nothing in the chain allowed the request (fail-closed default).
    NoMatch,
    /// An explicit `deny` rule matched.
    ExplicitDeny,
}

fn deny(req: &Request, cause: DenyCause) -> Decision {
    Decision::Deny(DenyReason {
        domain: req.domain(),
        value: req.value_string(),
        cause,
    })
}

// ── decision strategies (reusable across profiles) ──────────────────────────

/// Fail-closed and deny-dominant: any matching `deny` wins immediately
/// regardless of position; otherwise a matching `allow` permits; if nothing
/// matches, deny. This guarantees a more-specific level can only *narrow*.
pub fn deny_dominates<P: CapabilityProfile, R: OmniPathRoot>(
    rules: &[&Capability<P>],
    req: &Request,
    roots: &PathRoots<R>,
) -> Decision {
    let mut allowed = false;
    for c in rules {
        if rule_matches(&c.rule, req, roots) {
            match c.rule.access {
                Access::Deny => return deny(req, DenyCause::ExplicitDeny),
                Access::Allow => allowed = true,
            }
        }
    }
    if allowed {
        Decision::Allow
    } else {
        deny(req, DenyCause::NoMatch)
    }
}

/// Fail-closed, last-matching-rule wins. Order-sensitive: a later rule (from a
/// more-specific level) can flip an earlier decision either way.
pub fn last_match_wins<P: CapabilityProfile, R: OmniPathRoot>(
    rules: &[&Capability<P>],
    req: &Request,
    roots: &PathRoots<R>,
) -> Decision {
    let mut decision = deny(req, DenyCause::NoMatch);
    for c in rules {
        if rule_matches(&c.rule, req, roots) {
            decision = match c.rule.access {
                Access::Allow => Decision::Allow,
                Access::Deny => deny(req, DenyCause::ExplicitDeny),
            };
        }
    }
    decision
}

// ── evaluation ───────────────────────────────────────────────────────────────

/// Authorize a single request against the fully-cascaded chain: filter entries
/// that apply in `ctx`, then let the profile decide.
pub fn evaluate<P: CapabilityProfile, R: OmniPathRoot>(
    chain: &CapabilityRules<P>,
    req: &Request,
    roots: &PathRoots<R>,
    ctx: &P::Context,
) -> Decision {
    let applicable: Vec<&Capability<P>> = chain
        .iter()
        .filter(|c| P::applies(&c.applies_to, ctx))
        .collect();
    P::decide(&applicable, req, roots, ctx)
}

// ── shrink-only (attenuation) evaluation ─────────────────────────────────────

/// One level's local verdict on a request, in the shrink-only model.
///
/// A level constrains a *domain* only if it carries at least one rule for that
/// domain. Crucially, a level's `allow`-list plays **two** roles at once: it is
/// both a *grant* (it can be the level that authorizes the request) and a
/// *ceiling* (it caps this domain to exactly that list — anything outside it is
/// blocked, which is what prevents a deeper level from widening).
enum LevelOutcome {
    /// An explicit `deny` matched — dominant, refuses regardless of anything.
    Deny,
    /// The level whitelists this domain (`allow` rules present) but none matched
    /// the request: the request is outside this level's ceiling.
    Block,
    /// A matching `allow` and no matching `deny`: this level authorizes it.
    Grant,
    /// The level says nothing that constrains this domain (no `allow` rules, no
    /// matching `deny`): it neither grants nor caps — inherited authority flows
    /// through unchanged.
    Permit,
}

/// Classify a single level's stance on `req` (see [`LevelOutcome`]).
fn level_outcome<P: CapabilityProfile, R: OmniPathRoot>(
    level: &CapabilityRules<P>,
    req: &Request,
    roots: &PathRoots<R>,
    ctx: &P::Context,
) -> LevelOutcome {
    let domain = req.domain();
    let mut has_allow_for_domain = false;
    let mut matched_allow = false;
    for c in level.iter().filter(|c| P::applies(&c.applies_to, ctx)) {
        if c.rule.domain != domain {
            continue;
        }
        let matches = rule_matches(&c.rule, req, roots);
        match c.rule.access {
            Access::Deny if matches => return LevelOutcome::Deny,
            Access::Deny => {}
            Access::Allow => {
                has_allow_for_domain = true;
                matched_allow |= matches;
            }
        }
    }
    if matched_allow {
        LevelOutcome::Grant
    } else if has_allow_for_domain {
        LevelOutcome::Block
    } else {
        LevelOutcome::Permit
    }
}

/// Authorize a request under the **shrink-only (attenuation) model**: the
/// effective authority is the *intersection* of every level's own set, folded
/// outermost-first (`workspace ⊇ generator ⊇ … ⊇ action`, with each nested
/// `run-generator` inserting the parent's effective level ahead of the child).
///
/// A request is allowed **if**:
///
/// 1. no level explicitly denies it (deny-dominant), **and**
/// 2. no level blocks it — i.e. every level that whitelists the domain includes
///    it (the *ceiling* / attenuation rule: a deeper level can never reach
///    outside an upstream level's allow-list), **and**
/// 3. at least one level actively grants it (the *fail-closed* rule: a domain no
///    level allows is denied by default).
///
/// Adding a level can only keep the verdict or turn `Allow` into `Deny`; it can
/// never widen authority. Consequences that fall straight out of the three
/// rules:
///
/// * an **empty** level is `Permit` — pure pass-through, so an empty workspace
///   ceiling leaves a lone generator's policy `P` resolving to exactly `P`;
/// * a deeper `allow` for a path an upstream level did not allow is `Block`ed
///   upstream → **no escalation**;
/// * a `deny` at any level dominates every allow, at every other level.
///
/// `levels` are ordered outermost → innermost. An empty slice (or all-`Permit`
/// levels) yields a fail-closed deny because rule 3 is unmet — nothing granted.
///
/// NOTE — enforcement lowering: the coarse pre-spawn backends consume
/// [`project`], which still emits a per-domain allow/deny *list*. Under this
/// model the true effective set is an intersection that a flat list cannot
/// represent exactly, but that is safe: pre-spawn flags only need a conservative
/// **superset** (they must not block an allowed op), and the exact
/// per-operation floor is this function, run inside the broker. Reconciling
/// `project` with the fold (so flags lower the *narrowest* expressible bound) is
/// tracked in the shrink-only design ticket.
pub fn evaluate_layered<P: CapabilityProfile, R: OmniPathRoot>(
    levels: &[&CapabilityRules<P>],
    req: &Request,
    roots: &PathRoots<R>,
    ctx: &P::Context,
) -> Decision {
    let mut granted = false;
    let mut blocked = false;
    for level in levels {
        match level_outcome(level, req, roots, ctx) {
            // Explicit deny is dominant and the most informative reason.
            LevelOutcome::Deny => return deny(req, DenyCause::ExplicitDeny),
            LevelOutcome::Block => blocked = true,
            LevelOutcome::Grant => granted = true,
            LevelOutcome::Permit => {}
        }
    }
    if blocked || !granted {
        return deny(req, DenyCause::NoMatch);
    }
    Decision::Allow
}

/// Validate that every rule uses a domain the profile supports. Returns the
/// first offending entry as a hard error (fail-closed authoring).
pub fn validate<P: CapabilityProfile>(
    chain: &CapabilityRules<P>,
) -> Result<(), Error> {
    for (index, c) in chain.iter().enumerate() {
        if !P::supports(c.rule.domain) {
            return Err(Error::unsupported_domain(
                P::NAME,
                c.rule.domain,
                index,
            ));
        }
    }
    Ok(())
}

// ── projection to a backend-facing requirement ──────────────────────────────

/// An **opaque, planning-only surrogate key** for a distinct merged capability
/// atom — a `(domain, access, pattern)` triple after [`project`] deduplicates
/// it.
///
/// It exists solely so the enforcement planner can correlate the [`Gap`]s
/// different backends report against *the same* merged atom, and can look up
/// that atom's [`on_unenforceable`](CapabilityAtom::on_unenforceable) stance —
/// **by identity, not by re-deriving pattern strings**. That matters because
/// backends are free to normalize / resolve / split a pattern before reporting
/// a gap (an OS-sandbox backend lowering into SBPL filters or AppContainer
/// capability SIDs, say); once they do, correlating on the source string
/// silently breaks. Echoing this id instead removes that fragile invariant.
///
/// The value carries **no meaning** beyond "same atom": it is minted fresh by
/// each [`project`] call, is never serialized, never crosses the wire, and is
/// consumed only inside the planner. Domain-level analyses stay domain-keyed and
/// never touch it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(pub u32);

/// One distinct pattern within a domain's allow- or deny-list, carrying both its
/// opaque [`CapabilityId`] and the verbatim source `pattern` (kept for
/// diagnostics and gap rendering), plus the folded per-rule
/// [`on_unenforceable`](Self::on_unenforceable) stance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityAtom {
    /// Opaque surrogate key, unique across every atom in the owning
    /// [`RequiredCapabilities`]. Backends echo this into [`Gap`] so the planner
    /// can correlate gaps and resolve `on_unenforceable` without re-deriving
    /// strings.
    pub id: CapabilityId,
    /// The verbatim source pattern (fs glob, `host:port`, or name).
    pub pattern: String,
    /// The most-severe explicit per-rule choice for what to do if this atom
    /// turns out genuinely unenforceable, or `None` to defer to the caller's
    /// default (the fail-closed `deny`). Folded from every source rule that
    /// mentioned this pattern (`Allow` < `Warn` < `Deny`).
    pub on_unenforceable: Option<UnenforceablePolicy>,
}

/// The allow/deny [`CapabilityAtom`]s collected for one domain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainRules {
    pub allow: Vec<CapabilityAtom>,
    pub deny: Vec<CapabilityAtom>,
}

/// A neutral, ahead-of-time description of what an enforcement backend must set
/// up. The core emits this; it never decides whether a platform *can* enforce
/// it — that is the enforcement layer's job.
///
/// Because evaluation is fail-closed, **every supported domain is `restricted`**
/// (locked to its allow-list) unless a backend can prove otherwise.
///
/// The per-rule `on_unenforceable` stance is folded onto each
/// [`CapabilityAtom`] rather than kept in a separate side-map, so a backend that
/// reports a [`Gap`] echoing the atom's [`CapabilityId`] gives the planner a
/// single, cannot-miss lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequiredCapabilities {
    pub restricted: Vec<CapabilityDomain>,
    pub domains: BTreeMap<CapabilityDomain, DomainRules>,
}

/// Fold two `on_unenforceable` choices to the most severe (`Allow` < `Warn` <
/// `Deny`), treating `None` (defer-to-default) as the absence of a choice.
fn merge_on_unenforceable(
    a: Option<UnenforceablePolicy>,
    b: Option<UnenforceablePolicy>,
) -> Option<UnenforceablePolicy> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Accumulates one access side (allow or deny) of a domain: the patterns in
/// first-seen order, deduplicated, each with its folded `on_unenforceable`.
///
/// Deduping here is what guarantees exactly **one atom — hence one
/// [`CapabilityId`] — per distinct merged pattern**. The raw chain can (and
/// does, via `.extend`) mention the same pattern more than once; without this
/// dedup the same pattern would mint two ids and the planner's correlation would
/// double-count it.
#[derive(Default)]
struct SideBuilder {
    order: Vec<String>,
    policy: HashMap<String, Option<UnenforceablePolicy>>,
}

impl SideBuilder {
    fn add(&mut self, pattern: &str, on_unenf: Option<UnenforceablePolicy>) {
        use std::collections::hash_map::Entry;
        match self.policy.entry(pattern.to_string()) {
            Entry::Vacant(e) => {
                e.insert(on_unenf);
                self.order.push(pattern.to_string());
            }
            Entry::Occupied(mut e) => {
                let merged = merge_on_unenforceable(*e.get(), on_unenf);
                *e.get_mut() = merged;
            }
        }
    }

    /// Mint one atom per distinct pattern, drawing sequential ids from `next_id`
    /// (so `id == mint-order index`, making collisions structurally
    /// impossible).
    fn into_atoms(self, next_id: &mut u32) -> Vec<CapabilityAtom> {
        let SideBuilder { order, policy } = self;
        order
            .into_iter()
            .map(|pattern| {
                let on_unenforceable = policy.get(&pattern).copied().flatten();
                let id = CapabilityId(*next_id);
                *next_id += 1;
                CapabilityAtom {
                    id,
                    pattern,
                    on_unenforceable,
                }
            })
            .collect()
    }
}

#[derive(Default)]
struct DomainBuilder {
    allow: SideBuilder,
    deny: SideBuilder,
}

/// Lower the applicable-in-`ctx` slice of the chain into a
/// [`RequiredCapabilities`] description.
///
/// This is the **single mint site** for [`CapabilityId`]s: ids are assigned here
/// after the chain is merged and deduplicated, and everyone downstream is
/// read-only (backends only *echo* `atom.id`). Uniqueness across all atoms is
/// therefore a single-writer invariant with nothing to reconcile.
pub fn project<P: CapabilityProfile>(
    chain: &CapabilityRules<P>,
    ctx: &P::Context,
) -> RequiredCapabilities {
    // First pass: gather order-preserving, deduplicated patterns per
    // (domain, access), folding the most-severe `on_unenforceable` for repeats.
    // An omitted `on_unenforceable` is left as `None` (defer to the caller's
    // fail-closed default).
    let mut builders: BTreeMap<CapabilityDomain, DomainBuilder> =
        BTreeMap::new();
    for c in chain.iter().filter(|c| P::applies(&c.applies_to, ctx)) {
        let builder = builders.entry(c.rule.domain).or_default();
        let side = match c.rule.access {
            Access::Allow => &mut builder.allow,
            Access::Deny => &mut builder.deny,
        };
        for pattern in &c.rule.patterns {
            side.add(pattern, c.rule.on_unenforceable);
        }
    }

    // Second pass: mint an id per distinct atom, walking domains in BTreeMap
    // (sorted) order, allow before deny, patterns in first-seen order. The
    // deterministic walk is a nice-to-have for stable diagnostics/snapshots, not
    // required for correctness (each `project` mints fresh and all backends
    // share the one `RequiredCapabilities`).
    let mut next_id: u32 = 0;
    let mut domains: BTreeMap<CapabilityDomain, DomainRules> = BTreeMap::new();
    for (domain, builder) in builders {
        let allow = builder.allow.into_atoms(&mut next_id);
        let deny = builder.deny.into_atoms(&mut next_id);
        domains.insert(domain, DomainRules { allow, deny });
    }

    // Documents the single-writer uniqueness invariant.
    debug_assert!({
        let mut ids = std::collections::BTreeSet::new();
        domains
            .values()
            .flat_map(|r| r.allow.iter().chain(r.deny.iter()))
            .all(|a| ids.insert(a.id))
    });

    RequiredCapabilities {
        restricted: P::SUPPORTED.to_vec(),
        domains,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use merge::Merge as _;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{Capability, CapabilityProfile, CapabilityRules, NoExtra};

    fn roots() -> PathRoots {
        PathRoots::new().with(omni_types::Root::Workspace, "/repo")
    }

    // These exercise the profile-agnostic engine using the base `()` profile.

    fn parse(json: &str) -> CapabilityRules {
        serde_json::from_str(json).expect("valid capabilities config")
    }

    #[test]
    fn allow_within_root_deny_outside() {
        let cfg = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let inside = evaluate(
            &cfg,
            &Request::Fs {
                write: false,
                path: Path::new("/repo/src/a.rs"),
            },
            &roots(),
            &(),
        );
        assert!(inside.is_allowed());

        let outside = evaluate(
            &cfg,
            &Request::Fs {
                write: false,
                path: Path::new("/etc/passwd"),
            },
            &roots(),
            &(),
        );
        assert!(outside.is_denied());
    }

    #[test]
    fn deny_dominates_regardless_of_order() {
        // A later `allow` cannot re-open a `deny` from a broader level.
        let cfg = parse(
            r#"[
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }
            ]"#,
        );
        let d = evaluate(
            &cfg,
            &Request::Fs {
                write: true,
                path: Path::new("/repo/.git/config"),
            },
            &roots(),
            &(),
        );
        match d {
            Decision::Deny(r) => assert_eq!(r.cause, DenyCause::ExplicitDeny),
            other => panic!("expected explicit deny, got {other:?}"),
        }
    }

    #[test]
    fn last_match_wins_is_order_sensitive() {
        let cfg = parse(
            r#"[
                { "access": "deny",  "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/pkg/**"] }
            ]"#,
        );
        let rules: Vec<&Capability> = cfg.iter().collect();
        let req = Request::Fs {
            write: true,
            path: Path::new("/repo/pkg/x.rs"),
        };
        // last_match_wins: the trailing allow flips the earlier deny.
        assert!(last_match_wins(&rules, &req, &roots()).is_allowed());
        // deny_dominates: the deny still wins.
        assert!(deny_dominates(&rules, &req, &roots()).is_denied());
    }

    #[test]
    fn cascade_is_ordered_concatenation() {
        let mut workspace = parse(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }]"#,
        );
        let action = parse(
            r#"[{ "access": "deny", "domain": "fs.write", "patterns": ["@workspace/generated/**"] }]"#,
        );
        workspace.merge(action);
        assert_eq!(workspace.len(), 2);

        let denied = evaluate(
            &workspace,
            &Request::Fs {
                write: true,
                path: Path::new("/repo/generated/x.rs"),
            },
            &roots(),
            &(),
        );
        assert!(denied.is_denied());
    }

    #[test]
    fn project_collects_allow_and_deny_per_domain() {
        let cfg = parse(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.read",  "patterns": ["**/.env"] }
            ]"#,
        );
        let req = project(&cfg, &());
        let rules = &req.domains[&CapabilityDomain::FsRead];
        let allow: Vec<&str> =
            rules.allow.iter().map(|a| a.pattern.as_str()).collect();
        let deny: Vec<&str> =
            rules.deny.iter().map(|a| a.pattern.as_str()).collect();
        assert_eq!(allow, vec!["@workspace/**"]);
        assert_eq!(deny, vec!["**/.env"]);
        // Every atom carries a distinct opaque id.
        assert_ne!(rules.allow[0].id, rules.deny[0].id);
        // Fail-closed: every supported domain is locked down.
        assert_eq!(req.restricted.len(), CapabilityDomain::ALL.len());
    }

    #[test]
    fn project_dedups_a_repeated_pattern_to_one_atom() {
        // The same (domain, access, pattern) mentioned twice (as the raw
        // `.extend` chain does) must coalesce to a single atom — hence a single
        // id — not two.
        let cfg = parse(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["a:443"] },
                { "access": "allow", "domain": "net", "patterns": ["a:443", "b:443"] }
            ]"#,
        );
        let req = project(&cfg, &());
        let allow = &req.domains[&CapabilityDomain::Net].allow;
        let patterns: Vec<&str> =
            allow.iter().map(|a| a.pattern.as_str()).collect();
        // First-seen order preserved, `a:443` not duplicated.
        assert_eq!(patterns, vec!["a:443", "b:443"]);
    }

    #[test]
    fn project_folds_on_unenforceable_onto_the_atom_most_severe() {
        // Two rules govern the same atom with different stances; the atom keeps
        // the most severe (`warn` < `deny`). An atom with no explicit stance is
        // `None` (defers to the caller's default).
        let cfg = parse(
            r#"[
                { "access": "allow", "domain": "net", "patterns": ["a:443"], "on_unenforceable": "warn" },
                { "access": "allow", "domain": "net", "patterns": ["a:443"], "on_unenforceable": "deny" },
                { "access": "allow", "domain": "net", "patterns": ["b:443"] }
            ]"#,
        );
        let req = project(&cfg, &());
        let allow = &req.domains[&CapabilityDomain::Net].allow;
        let a = allow.iter().find(|x| x.pattern == "a:443").unwrap();
        let b = allow.iter().find(|x| x.pattern == "b:443").unwrap();
        assert_eq!(a.on_unenforceable, Some(UnenforceablePolicy::Deny));
        assert_eq!(b.on_unenforceable, None);
    }

    #[test]
    fn project_mints_a_unique_id_per_distinct_atom() {
        let cfg = parse(
            r#"[
                { "access": "allow", "domain": "fs.read",  "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.read",  "patterns": ["**/.env"] },
                { "access": "allow", "domain": "net",      "patterns": ["a:443", "b:443"] }
            ]"#,
        );
        let req = project(&cfg, &());
        let ids: Vec<CapabilityId> = req
            .domains
            .values()
            .flat_map(|r| r.allow.iter().chain(r.deny.iter()))
            .map(|a| a.id)
            .collect();
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(unique.len(), ids.len(), "ids must be unique");
    }

    // A local test-only profile exercising `SUPPORTED` and a custom `applies`,
    // proving the engine handles both without any concrete subsystem crate.

    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    struct Restricted;

    #[derive(
        Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema,
    )]
    struct TagScope {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
    }

    struct TagContext {
        tag: Option<String>,
    }

    impl CapabilityProfile for Restricted {
        const SUPPORTED: &'static [CapabilityDomain] =
            &[CapabilityDomain::FsRead, CapabilityDomain::FsWrite];
        const NAME: &'static str = "restricted";

        type AppliesTo = TagScope;
        type Extra = NoExtra;
        type Context = TagContext;

        fn applies(applies_to: &TagScope, ctx: &TagContext) -> bool {
            applies_to.tag.is_none() || applies_to.tag == ctx.tag
        }
    }

    #[test]
    fn validate_rejects_unsupported_domain() {
        let cfg: CapabilityRules<Restricted> = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        )
        .unwrap();
        let err = validate(&cfg).expect_err("net unsupported for `restricted`");
        assert_eq!(err.kind(), crate::ErrorKind::UnsupportedDomain);
    }

    #[test]
    fn applies_filters_entries_by_context() {
        let cfg: CapabilityRules<Restricted> = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["**"],
                  "applies_to": { "tag": "a" } }]"#,
        )
        .unwrap();
        let req = Request::Fs {
            write: false,
            path: Path::new("/x"),
        };

        let matching = evaluate(
            &cfg,
            &req,
            &roots(),
            &TagContext {
                tag: Some("a".into()),
            },
        );
        assert!(matching.is_allowed());

        let non_matching = evaluate(
            &cfg,
            &req,
            &roots(),
            &TagContext {
                tag: Some("b".into()),
            },
        );
        assert!(non_matching.is_denied(), "entry should be filtered out");
    }

    // ── shrink-only (attenuation) fold ──────────────────────────────────────

    fn fs_read(path: &str) -> Request<'_> {
        Request::Fs {
            write: false,
            path: Path::new(path),
        }
    }

    #[test]
    fn lone_level_is_fail_closed_on_an_unmentioned_domain() {
        // A single generator that only grants fs.read must still deny `net`,
        // exactly as the flat `evaluate` does today (rule 3: nothing granted).
        let p = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let levels: [&CapabilityRules; 1] = [&p];
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/a"), &roots(), &())
                .is_allowed()
        );
        let net = Request::Net {
            host: "example.com",
            port: 443,
        };
        assert!(
            evaluate_layered(&levels, &net, &roots(), &()).is_denied(),
            "an unmentioned domain must be fail-closed"
        );
    }

    #[test]
    fn empty_ceiling_is_pass_through() {
        // Empty workspace ceiling ⧺ generator P ⇒ resolves to exactly P.
        let empty = CapabilityRules::new();
        let p = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/src/**"] }]"#,
        );
        let levels: [&CapabilityRules; 2] = [&empty, &p];
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/src/a"), &roots(), &())
                .is_allowed()
        );
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/other/a"), &roots(), &())
                .is_denied(),
            "outside P's grant is denied even with an empty ceiling"
        );
    }

    #[test]
    fn a_deeper_level_cannot_widen_past_the_ceiling() {
        // THE security property: the workspace whitelists only `src`; a
        // generator that allows `secret` cannot reach it — the workspace level
        // Blocks it (attenuation), even though the generator granted it.
        let workspace = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/src/**"] }]"#,
        );
        let generator = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/secret/**"] }]"#,
        );
        let levels: [&CapabilityRules; 2] = [&workspace, &generator];
        assert!(
            evaluate_layered(
                &levels,
                &fs_read("/repo/secret/k"),
                &roots(),
                &()
            )
            .is_denied(),
            "generator must not escalate beyond the workspace ceiling"
        );
        // But within the shared ceiling it is allowed.
        let both = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/src/**"] }]"#,
        );
        let levels: [&CapabilityRules; 2] = [&workspace, &both];
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/src/a"), &roots(), &())
                .is_allowed()
        );
    }

    #[test]
    fn a_silent_level_passes_an_inherited_grant_through() {
        // Workspace grants `@ws/**`; a generator silent on fs neither caps nor
        // revokes, so the inherited grant flows through.
        let workspace = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let silent = CapabilityRules::new();
        let levels: [&CapabilityRules; 2] = [&workspace, &silent];
        assert!(
            evaluate_layered(
                &levels,
                &fs_read("/repo/anything"),
                &roots(),
                &()
            )
            .is_allowed()
        );
    }

    #[test]
    fn a_deny_at_any_level_dominates() {
        // A workspace deny of `.git` cannot be re-opened by a broad generator
        // allow — deny dominates across levels.
        let workspace = parse(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["**/.git/**"] }
            ]"#,
        );
        let generator = parse(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] }]"#,
        );
        let levels: [&CapabilityRules; 2] = [&workspace, &generator];
        let req = Request::Fs {
            write: true,
            path: Path::new("/repo/.git/config"),
        };
        match evaluate_layered(&levels, &req, &roots(), &()) {
            Decision::Deny(r) => assert_eq!(r.cause, DenyCause::ExplicitDeny),
            other => panic!("expected explicit deny, got {other:?}"),
        }
    }

    #[test]
    fn nested_confinement_narrows_at_each_level() {
        // workspace ⊇ parent ⊇ child, each strictly narrower. The child's
        // effective reach is the intersection: only `@ws/app/db`.
        let workspace = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let parent = parse(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/app/**"] }]"#,
        );
        let child = parse(
            r#"[
                { "access": "allow", "domain": "fs.read", "patterns": ["@workspace/app/db/**"] },
                { "access": "allow", "domain": "fs.read", "patterns": ["@workspace/other/**"] }
            ]"#,
        );
        let levels: [&CapabilityRules; 3] = [&workspace, &parent, &child];
        assert!(
            evaluate_layered(
                &levels,
                &fs_read("/repo/app/db/x"),
                &roots(),
                &()
            )
            .is_allowed()
        );
        // `other` is inside the workspace but outside the parent — Blocked.
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/other/x"), &roots(), &())
                .is_denied(),
            "child cannot re-grant what the parent narrowed away"
        );
        // `app/src` is inside workspace+parent but the child whitelisted only
        // `app/db` — Blocked by the child's own ceiling.
        assert!(
            evaluate_layered(
                &levels,
                &fs_read("/repo/app/src/x"),
                &roots(),
                &()
            )
            .is_denied()
        );
    }

    #[test]
    fn no_levels_is_fail_closed() {
        let levels: [&CapabilityRules; 0] = [];
        assert!(
            evaluate_layered(&levels, &fs_read("/repo/a"), &roots(), &())
                .is_denied(),
            "no level grants anything ⇒ fail-closed"
        );
    }
}
