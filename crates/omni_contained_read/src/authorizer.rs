use std::{path::PathBuf, sync::Arc};

use omni_capabilities::{Decision, DenyCause as PolicyCause, Request};
use omni_capability_sys::CapabilityAuthorizer;
use omni_input_view::{
    AuthDecision, AuthError, DenyCause, DenyReason, InputAuthorizer,
};
use omni_path_utils::WorkspacePath;

/// One authorizer shared by the broker and the view's adapter.
///
/// `EvaluatingAuthorizer` is not `Clone` and `PolicyEnforcingSys` consumes its
/// authorizer, so the runner builds it once and hands each side a clone of this
/// handle.
pub struct SharedAuthorizer<A>(pub Arc<A>);

impl<A> SharedAuthorizer<A> {
    pub fn new(authorizer: A) -> Self {
        Self(Arc::new(authorizer))
    }
}

impl<A> Clone for SharedAuthorizer<A> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<A: CapabilityAuthorizer> CapabilityAuthorizer for SharedAuthorizer<A> {
    fn authorize(&self, request: &Request<'_>) -> Decision {
        self.0.authorize(request)
    }
}

/// Adapts the capability policy to the view's `InputAuthorizer`.
///
/// The request path is the workspace root joined with the validated suffix
/// (symlinks are rejected, so the lexical path is the path that is read). A
/// root that is not an absolute UTF-8 path cannot be turned into a request and
/// is `AuthError`, never an allow.
pub struct CapabilityInputAuthorizer<A> {
    authorizer: A,
    root: PathBuf,
}

impl<A> CapabilityInputAuthorizer<A> {
    pub fn new(authorizer: A, root: PathBuf) -> Self {
        Self { authorizer, root }
    }

    fn join(&self, path: &WorkspacePath) -> Result<PathBuf, AuthError> {
        if !self.root.is_absolute() || self.root.to_str().is_none() {
            return Err(AuthError);
        }
        let mut joined = self.root.clone();
        if !path.is_root() {
            joined.extend(path.segments());
        }
        Ok(joined)
    }
}

impl<A: CapabilityAuthorizer> InputAuthorizer for CapabilityInputAuthorizer<A> {
    fn authorize_read(
        &self,
        path: &WorkspacePath,
    ) -> Result<AuthDecision, AuthError> {
        let joined = self.join(path)?;
        Ok(
            match self.authorizer.authorize(&Request::Fs {
                write: false,
                path: &joined,
            }) {
                Decision::Allow => AuthDecision::Allow,
                Decision::Deny(reason) => AuthDecision::Deny(DenyReason {
                    path: path.clone(),
                    cause: match reason.cause {
                        PolicyCause::NoMatch => DenyCause::NoMatch,
                        PolicyCause::ExplicitDeny => DenyCause::ExplicitDeny,
                    },
                }),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use omni_capabilities::{CapabilityRules, PathRoots, Root};
    use omni_capability_sys::EvaluatingAuthorizer;

    use super::*;

    fn caps(json: &str) -> CapabilityRules {
        serde_json::from_str(json).expect("valid capability chain")
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join("omni-contained-read-root")
    }

    fn roots() -> PathRoots<Root> {
        PathRoots::new().with(Root::Workspace, root())
    }

    fn wp(path: &str) -> WorkspacePath {
        WorkspacePath::parse(path).unwrap()
    }

    const ALLOW: &str =
        r#"{"access":"allow","domain":"fs.read","patterns":["@workspace/**"]}"#;
    const DENY: &str = r#"{"access":"deny","domain":"fs.read","patterns":["@workspace/secret/**"]}"#;

    fn adapter(
        levels: Vec<CapabilityRules>,
    ) -> CapabilityInputAuthorizer<SharedAuthorizer<EvaluatingAuthorizer<()>>>
    {
        CapabilityInputAuthorizer::new(
            SharedAuthorizer::new(EvaluatingAuthorizer::layered(
                levels,
                roots(),
                (),
            )),
            root(),
        )
    }

    #[test]
    fn allow_explicit_deny_and_no_match_map_to_the_view_decisions() {
        let a = adapter(vec![caps(&format!("[{ALLOW},{DENY}]"))]);
        assert_eq!(
            a.authorize_read(&wp("src/a")).unwrap(),
            AuthDecision::Allow
        );
        assert_eq!(
            a.authorize_read(&wp("secret/key")).unwrap(),
            AuthDecision::Deny(DenyReason {
                path: wp("secret/key"),
                cause: DenyCause::ExplicitDeny
            })
        );
        let none = adapter(vec![caps("[]")]);
        assert_eq!(
            none.authorize_read(&wp("src/a")).unwrap(),
            AuthDecision::Deny(DenyReason {
                path: wp("src/a"),
                cause: DenyCause::NoMatch
            })
        );
    }

    #[test]
    fn a_deeper_level_cannot_widen_past_an_upstream_ceiling() {
        let ceiling = caps(
            r#"[{"access":"allow","domain":"fs.read","patterns":["@workspace/src/**"]}]"#,
        );
        let inner = caps(&format!("[{ALLOW}]"));
        let a = adapter(vec![ceiling, inner]);
        assert_eq!(
            a.authorize_read(&wp("src/a")).unwrap(),
            AuthDecision::Allow
        );
        assert!(matches!(
            a.authorize_read(&wp("other/a")).unwrap(),
            AuthDecision::Deny(_)
        ));
    }

    #[test]
    fn the_request_path_is_the_root_joined_with_the_suffix() {
        struct Capture(std::sync::Mutex<Vec<PathBuf>>);
        impl CapabilityAuthorizer for Capture {
            fn authorize(&self, request: &Request<'_>) -> Decision {
                if let Request::Fs { write, path } = request {
                    assert!(!write);
                    self.0.lock().unwrap().push(path.to_path_buf());
                }
                Decision::Allow
            }
        }
        let capture = Capture(Default::default());
        let a = CapabilityInputAuthorizer::new(capture, root());
        a.authorize_read(&wp("a/b/c")).unwrap();
        a.authorize_read(&WorkspacePath::root()).unwrap();
        let seen = a.authorizer.0.lock().unwrap().clone();
        assert_eq!(seen, vec![root().join("a").join("b").join("c"), root()]);
    }

    #[test]
    fn a_root_that_cannot_build_a_request_is_an_error_never_an_allow() {
        let a = CapabilityInputAuthorizer::new(
            SharedAuthorizer::new(EvaluatingAuthorizer::new(
                caps(&format!("[{ALLOW}]")),
                roots(),
                (),
            )),
            Path::new("relative/root").to_path_buf(),
        );
        assert_eq!(a.authorize_read(&wp("a")), Err(AuthError));
    }
}
