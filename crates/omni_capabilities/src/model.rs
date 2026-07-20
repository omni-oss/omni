//! Core data model: the platform-neutral, subsystem-agnostic capability types.
//!
//! Nothing in this module knows about operating systems, sandbox mechanisms,
//! concrete script runtimes, or which subsystems exist. It only describes
//! *policy*: ordered `allow`/`deny` rules over abstract [`CapabilityDomain`]s.
//! The shape of each entry's `applies_to` selector is owned entirely by the
//! active [`CapabilityProfile`](crate::CapabilityProfile).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::VariantArray as _;

use crate::CapabilityProfile;

/// An abstract capability domain — the universal vocabulary shared by every
/// subsystem and every enforcement backend.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
    strum::Display,
    strum::VariantArray,
)]
pub enum CapabilityDomain {
    /// Reading files / directories.
    #[serde(rename = "fs.read")]
    #[strum(serialize = "fs.read")]
    FsRead,
    /// Creating / writing / removing files and directories.
    #[serde(rename = "fs.write")]
    #[strum(serialize = "fs.write")]
    FsWrite,
    /// Outbound network access (matched as `host:port`).
    #[serde(rename = "net")]
    #[strum(serialize = "net")]
    Net,
    /// Reading environment variables.
    #[serde(rename = "env")]
    #[strum(serialize = "env")]
    Env,
    /// Spawning child processes.
    #[serde(rename = "process")]
    #[strum(serialize = "process")]
    Process,
}

impl CapabilityDomain {
    /// Every domain, in a stable order. Used as the default `SUPPORTED` set.
    pub const ALL: &'static [CapabilityDomain] = Self::VARIANTS;
}

/// Whether a rule grants or revokes access.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Access {
    /// Permit the matching operation.
    Allow,
    /// Refuse the matching operation.
    Deny,
}

/// What to do when a rule's pattern turns out to be **genuinely unenforceable**
/// — i.e. no selected enforcement backend (pre-spawn flags, OS sandbox, or
/// in-process broker) can faithfully confine it on the current platform.
///
/// This is a *policy* decision owned by the author of the rule, not by any
/// particular enforcement mechanism, which is why it lives in the core model
/// even though the core itself never acts on it: the enforcement layer reads it
/// when composing a plan (see `omni_capability_enforcement`).
///
/// Variants are ordered by severity (`Allow` < `Warn` < `Deny`) so that when the
/// same pattern is governed by several rules, the most severe choice wins.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum UnenforceablePolicy {
    /// Proceed silently, dropping the unenforceable pattern (least confinement).
    Allow,
    /// Proceed, but surface a warning for the pattern that will not be enforced.
    Warn,
    /// Refuse to run and report the unenforceable pattern. **Default** — if we
    /// cannot confine it, we do not run it.
    #[default]
    Deny,
}

/// The subsystem-neutral body of a single capability entry.
///
/// `patterns` are platform-neutral: filesystem globs (`@workspace/**`),
/// `host:port` for [`Net`](CapabilityDomain::Net), or plain names/globs for
/// [`Env`](CapabilityDomain::Env) / [`Process`](CapabilityDomain::Process).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRule {
    /// Whether this rule grants (`allow`) or revokes (`deny`) access.
    pub access: Access,
    pub domain: CapabilityDomain,
    /// Neutral patterns matched against the request (fs globs, `host:port`, or
    /// names). An empty list matches nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    /// What to do if this rule cannot be faithfully enforced on the current
    /// platform. `None` inherits the caller's default (the fail-closed `deny`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_unenforceable: Option<UnenforceablePolicy>,
}

/// A single capability entry: what it applies to + the rule it expresses.
///
/// The `applies_to` selector is whatever the profile defines
/// ([`CapabilityProfile::AppliesTo`]); the core imposes no shared fields on it,
/// so subsystem concepts (which subsystem, which action, which runtime, …) live
/// in the profile, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "P::AppliesTo: Serialize, P::Extra: Serialize",
    deserialize = "P::AppliesTo: serde::Deserialize<'de>, P::Extra: serde::Deserialize<'de>"
))]
pub struct Capability<P: CapabilityProfile = ()> {
    /// Profile-defined selector for what this entry applies to.
    #[serde(default)]
    pub applies_to: P::AppliesTo,

    /// Per-entry profile extras (empty for most profiles).
    #[serde(flatten)]
    pub extra: P::Extra,

    /// The allow/deny rule this entry expresses.
    #[serde(flatten)]
    pub rule: CapabilityRule,
}

/// The ordered list of capability rules attached at each level (workspace,
/// project, generator, action, …). It is *the* artifact: a transparent, ordered
/// array of rules. Cascading is plain concatenation in declaration order (see
/// the [`merge::Merge`] impl), with folding behavior chosen by the profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[serde(bound(
    serialize = "P::AppliesTo: Serialize, P::Extra: Serialize",
    deserialize = "P::AppliesTo: serde::Deserialize<'de>, P::Extra: serde::Deserialize<'de>"
))]
pub struct CapabilityRules<P: CapabilityProfile = ()>(pub Vec<Capability<P>>);

impl<P: CapabilityProfile> Default for CapabilityRules<P> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<P: CapabilityProfile> CapabilityRules<P> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_rules(rules: impl IntoIterator<Item = Capability<P>>) -> Self {
        Self(rules.into_iter().collect())
    }

    pub fn push(&mut self, rule: Capability<P>) {
        self.0.push(rule);
    }

    pub fn as_slice(&self) -> &[Capability<P>] {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Capability<P>> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<Src: CapabilityProfile> CapabilityRules<Src> {
    /// Reinterpret this config's rules under a different profile `Dst`, deciding
    /// each entry's fate with an explicit `map`:
    ///
    /// * `Some(applies_to)` — keep the entry under `Dst` with that selector,
    /// * `None` — drop it (filtered out).
    ///
    /// The rule body (`access`/`domain`/`patterns`) is copied verbatim; the
    /// entry's `extra` is reset to `Dst`'s default. Because the caller must
    /// return the destination selector for every entry it keeps, a source
    /// selector can never be *silently* dropped — discarding scope is always an
    /// explicit choice, so authority is never accidentally widened.
    ///
    /// This is the mechanism behind workspace capabilities: a single flat list
    /// tagged by subsystem is filtered-and-folded into each subsystem's typed
    /// cascade (keep the entries whose tag includes that subsystem; map their
    /// selector to the subsystem's default).
    pub fn reinterpret<Dst, F>(self, mut map: F) -> CapabilityRules<Dst>
    where
        Dst: CapabilityProfile,
        F: FnMut(&Src::AppliesTo) -> Option<Dst::AppliesTo>,
    {
        CapabilityRules(
            self.0
                .into_iter()
                .filter_map(|c| {
                    map(&c.applies_to).map(|applies_to| Capability {
                        applies_to,
                        extra: Dst::Extra::default(),
                        rule: c.rule,
                    })
                })
                .collect(),
        )
    }
}

impl CapabilityRules<()> {
    /// Reinterpret subsystem-agnostic (base-profile) rules under a concrete
    /// subsystem profile `P`, giving each entry `P`'s default `applies_to`
    /// — i.e. "applies everywhere within that subsystem". A total upcast (every
    /// entry is kept), which is safe precisely because the base `()` profile has
    /// no selector to lose.
    ///
    /// Whether every `domain` is expressible by `P` is a separate concern for
    /// [`validate`](crate::validate).
    pub fn into_profile<P: CapabilityProfile>(self) -> CapabilityRules<P> {
        self.reinterpret(|_| Some(P::AppliesTo::default()))
    }
}

impl<P: CapabilityProfile> merge::Merge for CapabilityRules<P> {
    /// Cascade = concatenation in declaration order
    /// (workspace ⧺ project ⧺ unit ⧺ action). Each incoming entry is folded in
    /// through [`CapabilityProfile::merge_entry`], so a profile can dedup or
    /// coalesce if it wants; the default simply appends.
    fn merge(&mut self, other: Self) {
        for entry in other.0 {
            P::merge_entry(&mut self.0, entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{CapabilityProfile, NoExtra};

    /// A scoped selector with a meaningful default, to prove the upcast fills
    /// in `P`'s defaults rather than leaving anything unset.
    #[derive(
        Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema,
    )]
    struct Scope {
        #[serde(default)]
        tags: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    struct Scoped;

    impl CapabilityProfile for Scoped {
        type AppliesTo = Scope;
        type Extra = NoExtra;
        type Context = ();
    }

    fn base_rule(
        access: Access,
        domain: CapabilityDomain,
        pattern: &str,
    ) -> Capability {
        Capability {
            applies_to: NoExtra {},
            extra: NoExtra {},
            rule: CapabilityRule {
                access,
                domain,
                patterns: vec![pattern.to_string()],
                on_unenforceable: None,
            },
        }
    }

    #[test]
    fn into_profile_copies_rules_and_defaults_the_selector() {
        let base = CapabilityRules::from_rules([
            base_rule(Access::Allow, CapabilityDomain::FsRead, "@workspace/**"),
            base_rule(Access::Deny, CapabilityDomain::FsWrite, "**/.git/**"),
        ]);

        let scoped: CapabilityRules<Scoped> = base.into_profile();

        assert_eq!(scoped.len(), 2);
        // Rules are preserved verbatim.
        assert_eq!(scoped.as_slice()[0].rule.domain, CapabilityDomain::FsRead);
        assert_eq!(
            scoped.as_slice()[0].rule.patterns,
            vec!["@workspace/**".to_string()]
        );
        assert_eq!(scoped.as_slice()[1].rule.access, Access::Deny);
        // The selector is `P`'s default (applies everywhere within the subsystem).
        assert_eq!(scoped.as_slice()[0].applies_to, Scope::default());
    }

    #[test]
    fn reinterpret_drops_entries_the_closure_rejects() {
        // A `Scoped` source whose selector carries tags; reinterpret into the
        // base profile, keeping only entries tagged "keep". This is the shape of
        // filtering a subsystem-tagged workspace list down to one subsystem.
        fn scoped_rule(
            domain: CapabilityDomain,
            tag: &str,
        ) -> Capability<Scoped> {
            Capability {
                applies_to: Scope {
                    tags: vec![tag.to_string()],
                },
                extra: NoExtra {},
                rule: CapabilityRule {
                    access: Access::Allow,
                    domain,
                    patterns: vec!["**".to_string()],
                    on_unenforceable: None,
                },
            }
        }

        let src = CapabilityRules::<Scoped>::from_rules([
            scoped_rule(CapabilityDomain::FsRead, "keep"),
            scoped_rule(CapabilityDomain::FsWrite, "other"),
            scoped_rule(CapabilityDomain::Process, "keep"),
        ]);

        let kept: CapabilityRules = src.reinterpret(|scope| {
            scope.tags.iter().any(|t| t == "keep").then_some(NoExtra {})
        });

        assert_eq!(kept.len(), 2, "only the two `keep` entries survive");
        assert_eq!(kept.as_slice()[0].rule.domain, CapabilityDomain::FsRead);
        assert_eq!(kept.as_slice()[1].rule.domain, CapabilityDomain::Process);
    }
}
