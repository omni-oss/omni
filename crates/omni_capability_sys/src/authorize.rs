//! The authorization seam: how the sys decorator asks "is this operation
//! allowed?" without knowing anything about profiles, cascades, or roots.
//!
//! [`PolicyEnforcingSys`](crate::PolicyEnforcingSys) builds a [`Request`] for
//! each intercepted syscall and hands it to a [`CapabilityAuthorizer`]. The
//! standard implementation, [`EvaluatingAuthorizer`], evaluates that request
//! against an ordered set of policy [`CapabilityRules`] levels using the
//! fail-closed, shrink-only engine in `omni_capabilities` (each level can only
//! narrow the authority it inherited); but callers are free to supply their own
//! (for tests, auditing, or an entirely different policy source).

use omni_capabilities::{
    CapabilityProfile, CapabilityRules, Decision, OmniPathRoot, PathRoots,
    Request, Root, evaluate_layered,
};

/// Decides whether a single operation is permitted.
///
/// This is deliberately tiny and object-safe so the decorator stays agnostic to
/// *how* the decision is made.
pub trait CapabilityAuthorizer: Send + Sync {
    fn authorize(&self, request: &Request<'_>) -> Decision;
}

/// The standard authorizer: evaluate each request against a cascaded policy
/// under the **shrink-only (attenuation) model**.
///
/// It owns the immutable inputs to [`evaluate_layered`] — the ordered rule
/// `levels` (outermost → innermost: workspace, ancestor generators, this
/// generator, this action), the [`PathRoots`] used to resolve `@root/...`
/// patterns, and the profile `context` (e.g. the current generator
/// action/target) — so that each authorization is a pure lookup.
///
/// Each level can only ever *narrow* the authority it inherited: a request is
/// allowed iff no level denies it, no level with a whitelist for the domain
/// leaves it out (the ceiling / attenuation rule), and at least one level grants
/// it (fail-closed). A single-level authorizer (the common case, built via
/// [`new`](Self::new)) degrades to the classic deny-dominant behaviour, since a
/// lone level's grant is its own ceiling.
pub struct EvaluatingAuthorizer<P: CapabilityProfile, R: OmniPathRoot = Root> {
    levels: Vec<CapabilityRules<P>>,
    roots: PathRoots<R>,
    context: P::Context,
}

impl<P: CapabilityProfile, R: OmniPathRoot> EvaluatingAuthorizer<P, R> {
    /// A single-level authorizer: the `chain` is treated as one policy level.
    /// Because a lone level is both grant and ceiling, this matches the
    /// pre-attenuation deny-dominant semantics exactly.
    pub fn new(
        chain: CapabilityRules<P>,
        roots: PathRoots<R>,
        context: P::Context,
    ) -> Self {
        Self::layered(vec![chain], roots, context)
    }

    /// A layered authorizer: `levels` are ordered outermost → innermost and each
    /// can only shrink the authority inherited from the levels ahead of it (see
    /// [`evaluate_layered`]).
    pub fn layered(
        levels: Vec<CapabilityRules<P>>,
        roots: PathRoots<R>,
        context: P::Context,
    ) -> Self {
        Self {
            levels,
            roots,
            context,
        }
    }
}

impl<P, R> CapabilityAuthorizer for EvaluatingAuthorizer<P, R>
where
    P: CapabilityProfile,
    R: OmniPathRoot + Send + Sync,
    CapabilityRules<P>: Send + Sync,
    P::Context: Send + Sync,
{
    fn authorize(&self, request: &Request<'_>) -> Decision {
        let levels: Vec<&CapabilityRules<P>> = self.levels.iter().collect();
        evaluate_layered(&levels, request, &self.roots, &self.context)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn caps(json: &str) -> CapabilityRules {
        serde_json::from_str(json).expect("valid capability chain")
    }

    fn roots() -> PathRoots<Root> {
        PathRoots::new().with(Root::Workspace, "/repo")
    }

    fn read(path: &str) -> Request<'_> {
        Request::Fs {
            write: false,
            path: Path::new(path),
        }
    }

    #[test]
    fn single_level_is_deny_dominant() {
        let auth = EvaluatingAuthorizer::new(
            caps(
                r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
            ),
            roots(),
            (),
        );
        assert!(auth.authorize(&read("/repo/src/a")).is_allowed());
        assert!(auth.authorize(&read("/etc/passwd")).is_denied());
    }

    #[test]
    fn a_deeper_level_cannot_widen_past_an_upstream_ceiling() {
        // Outermost level caps reads to `@workspace/src/**`; the inner level
        // tries to grant itself all of `@workspace/**`. Attenuation must keep
        // the effective set to the intersection (`src`), so a read of
        // `@workspace/secret` is blocked even though the inner level allowed it.
        let ceiling = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/src/**"] }]"#,
        );
        let inner = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let auth =
            EvaluatingAuthorizer::layered(vec![ceiling, inner], roots(), ());

        assert!(
            auth.authorize(&read("/repo/src/a")).is_allowed(),
            "a read inside the intersection is allowed"
        );
        assert!(
            auth.authorize(&read("/repo/secret")).is_denied(),
            "a deeper level must not escalate beyond the upstream ceiling"
        );
    }
}
