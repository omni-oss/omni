//! The isolated scope label a run asks for (RFC 0030, section 4.4).
//!
//! The label only names a partition inside the caller's own isolated
//! namespace. It is not authenticated, and a scope pin on the credential
//! overrides it on the server.

use omni_configurations::ScopeConfig;
use sha2::{Digest as _, Sha256};

const MAX_LABEL: usize = 64;
const PREFIX_LEN: usize = 55;

pub(crate) const DEFAULT_LABEL: &str = "default";

fn hash8(raw: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(raw.as_bytes()).iter().take(4).fold(
        String::new(),
        |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        },
    )
}

/// Makes `raw` a label the server accepts
/// (`[A-Za-z0-9][A-Za-z0-9._-]{0,63}`). Text that already fits is kept; text
/// that had to change, or is too long, becomes a prefix, `-` and 8 hex
/// characters of the SHA-256 of the original, so two long or odd branch names
/// stay distinct. An empty value is `None`.
pub(crate) fn sanitize(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let mut label: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut changed = label != raw;
    if !label.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        label.insert(0, 'x');
        changed = true;
    }
    if label.len() > MAX_LABEL {
        changed = true;
    }
    if !changed {
        return Some(label);
    }
    label.truncate(PREFIX_LEN);
    Some(format!("{label}-{}", hash8(raw)))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn digits(value: &str) -> Option<&str> {
    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        .then_some(value)
}

/// The pull request number and branch of the current CI run, if any.
struct CiRun {
    pull_request: Option<String>,
    branch: Option<String>,
    run: Option<String>,
}

fn detect(env: &dyn Fn(&str) -> Option<String>) -> Option<CiRun> {
    let get = |name: &str| non_empty(env(name));
    if get("GITHUB_ACTIONS").is_some() {
        let pull_request = get("GITHUB_REF").and_then(|r| {
            r.strip_prefix("refs/pull/")
                .and_then(|rest| rest.split('/').next())
                .and_then(digits)
                .map(str::to_owned)
        });
        return Some(CiRun {
            pull_request,
            branch: get("GITHUB_HEAD_REF").or_else(|| get("GITHUB_REF_NAME")),
            run: get("GITHUB_RUN_ID"),
        });
    }
    if get("GITLAB_CI").is_some() {
        return Some(CiRun {
            pull_request: get("CI_MERGE_REQUEST_IID")
                .filter(|n| digits(n).is_some()),
            branch: get("CI_COMMIT_REF_SLUG"),
            run: get("CI_PIPELINE_ID"),
        });
    }
    if get("CIRCLECI").is_some() {
        let pull_request = get("CIRCLE_PULL_REQUEST").and_then(|url| {
            url.rsplit('/').next().and_then(digits).map(str::to_owned)
        });
        return Some(CiRun {
            pull_request,
            branch: get("CIRCLE_BRANCH"),
            run: get("CIRCLE_WORKFLOW_ID"),
        });
    }
    if get("BUILDKITE").is_some() {
        return Some(CiRun {
            pull_request: get("BUILDKITE_PULL_REQUEST")
                .filter(|n| digits(n).is_some()),
            branch: get("BUILDKITE_BRANCH"),
            run: get("BUILDKITE_BUILD_ID"),
        });
    }
    if get("JENKINS_URL").is_some() {
        let run = match (get("JOB_NAME"), get("BUILD_NUMBER")) {
            (Some(job), Some(number)) => Some(format!("{job}#{number}")),
            _ => None,
        };
        return Some(CiRun {
            pull_request: get("CHANGE_ID").filter(|n| digits(n).is_some()),
            branch: get("BRANCH_NAME"),
            run,
        });
    }
    None
}

/// The label for `config`. The second value is a warning for the caller to
/// log when a request could not be honoured and `default` was used.
pub(crate) fn resolve(
    config: &ScopeConfig,
    env: &dyn Fn(&str) -> Option<String>,
) -> (String, Option<&'static str>) {
    let default = || DEFAULT_LABEL.to_owned();
    match config {
        ScopeConfig::Literal(label) => (label.clone(), None),
        ScopeConfig::Auto => {
            let label = detect(env).and_then(|ci| {
                ci.pull_request
                    .map(|n| format!("pr-{n}"))
                    .or_else(|| ci.branch.as_deref().and_then(sanitize))
            });
            (label.unwrap_or_else(default), None)
        }
        ScopeConfig::Run => {
            match detect(env)
                .and_then(|ci| ci.run)
                .as_deref()
                .and_then(sanitize)
            {
                Some(label) => (label, None),
                None => (
                    default(),
                    Some("`scope: run` needs a CI run id; using `default`"),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(
        vars: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    fn auto(vars: &[(&str, &str)]) -> String {
        resolve(&ScopeConfig::Auto, &lookup(vars)).0
    }

    #[test]
    fn labels_that_fit_are_kept_and_others_get_a_distinguishing_suffix() {
        assert_eq!(sanitize("main").as_deref(), Some("main"));
        assert_eq!(sanitize("Release_1.2").as_deref(), Some("Release_1.2"));
        assert_eq!(sanitize(""), None);
        let slash = sanitize("feature/a").unwrap();
        let other = sanitize("feature-a").unwrap();
        assert_eq!(other, "feature-a");
        assert!(slash.starts_with("feature-a-") && slash != other);
        let dotted = sanitize(".hidden").unwrap();
        assert!(dotted.starts_with("x.hidden-"), "{dotted}");
        for raw in [
            "..",
            "a/b",
            "é",
            "-x",
            "a b",
            "feature/\u{202e}evil",
            &"b".repeat(200),
        ] {
            let label = sanitize(raw).unwrap();
            assert!(label.len() <= MAX_LABEL, "{label}");
            assert!(
                label.starts_with(|c: char| c.is_ascii_alphanumeric()),
                "{label}"
            );
            assert!(
                label.bytes().all(|b| b.is_ascii_alphanumeric()
                    || matches!(b, b'.' | b'_' | b'-')),
                "{label}"
            );
        }
        // Two long names that share a prefix stay distinct, and the exact
        // limit is kept as is.
        assert_ne!(
            sanitize(&format!("{}a", "b".repeat(100))),
            sanitize(&format!("{}c", "b".repeat(100)))
        );
        assert_eq!(sanitize(&"b".repeat(64)).unwrap().len(), 64);
        assert_ne!(sanitize(&"b".repeat(64)), sanitize(&"b".repeat(65)));
    }

    #[test]
    fn outside_ci_the_label_is_default() {
        assert_eq!(auto(&[]), "default");
        assert_eq!(auto(&[("GITHUB_REF", "refs/pull/1/merge")]), "default");
    }

    #[test]
    fn github_uses_the_pull_request_number_then_the_branch() {
        let ci = ("GITHUB_ACTIONS", "true");
        assert_eq!(
            auto(&[
                ci,
                ("GITHUB_REF", "refs/pull/42/merge"),
                ("GITHUB_HEAD_REF", "x")
            ]),
            "pr-42"
        );
        // pull_request_target and pushes have no PR number in the ref.
        assert_eq!(
            auto(&[
                ci,
                ("GITHUB_REF", "refs/heads/main"),
                ("GITHUB_HEAD_REF", "topic")
            ]),
            "topic"
        );
        assert_eq!(
            auto(&[
                ci,
                ("GITHUB_REF", "refs/heads/main"),
                ("GITHUB_REF_NAME", "main"),
                ("GITHUB_HEAD_REF", "")
            ]),
            "main"
        );
        // Not a number: ignored.
        assert_eq!(
            auto(&[
                ci,
                ("GITHUB_REF", "refs/pull/x/merge"),
                ("GITHUB_REF_NAME", "m")
            ]),
            "m"
        );
        assert_eq!(
            auto(&[
                ci,
                ("GITHUB_REF", "refs/tags/v1"),
                ("GITHUB_REF_NAME", "v1")
            ]),
            "v1"
        );
    }

    #[test]
    fn the_other_providers() {
        assert_eq!(
            auto(&[
                ("GITLAB_CI", "true"),
                ("CI_MERGE_REQUEST_IID", "7"),
                ("CI_COMMIT_REF_SLUG", "b")
            ]),
            "pr-7"
        );
        assert_eq!(
            auto(&[("GITLAB_CI", "true"), ("CI_COMMIT_REF_SLUG", "fix-it")]),
            "fix-it"
        );
        assert_eq!(
            auto(&[
                ("CIRCLECI", "true"),
                ("CIRCLE_PULL_REQUEST", "https://github.com/o/r/pull/19"),
                ("CIRCLE_BRANCH", "b")
            ]),
            "pr-19"
        );
        assert_eq!(
            auto(&[("CIRCLECI", "true"), ("CIRCLE_BRANCH", "dev")]),
            "dev"
        );
        assert_eq!(
            auto(&[
                ("BUILDKITE", "true"),
                ("BUILDKITE_PULL_REQUEST", "false"),
                ("BUILDKITE_BRANCH", "main")
            ]),
            "main"
        );
        assert_eq!(
            auto(&[("BUILDKITE", "true"), ("BUILDKITE_PULL_REQUEST", "5")]),
            "pr-5"
        );
        assert_eq!(
            auto(&[
                ("JENKINS_URL", "http://j"),
                ("CHANGE_ID", "3"),
                ("BRANCH_NAME", "PR-3")
            ]),
            "pr-3"
        );
        assert_eq!(
            auto(&[("JENKINS_URL", "http://j"), ("BRANCH_NAME", "main")]),
            "main"
        );
    }

    #[test]
    fn the_first_detected_provider_wins() {
        assert_eq!(
            auto(&[
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_REF_NAME", "gh"),
                ("GITLAB_CI", "true"),
                ("CI_COMMIT_REF_SLUG", "gl")
            ]),
            "gh"
        );
    }

    #[test]
    fn a_hostile_branch_name_cannot_make_an_invalid_label() {
        let label = auto(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_HEAD_REF", "../../etc/passwd"),
        ]);
        assert!(label.starts_with("x.."), "{label}");
        assert!(!label.contains('/'));
    }

    #[test]
    fn run_uses_the_run_id_and_warns_outside_ci() {
        let (label, warning) = resolve(
            &ScopeConfig::Run,
            &lookup(&[("GITHUB_ACTIONS", "true"), ("GITHUB_RUN_ID", "123456")]),
        );
        assert_eq!((label.as_str(), warning), ("123456", None));
        let (label, warning) = resolve(&ScopeConfig::Run, &lookup(&[]));
        assert_eq!(label, "default");
        assert!(warning.is_some());
        let (label, _) = resolve(
            &ScopeConfig::Run,
            &lookup(&[
                ("JENKINS_URL", "u"),
                ("JOB_NAME", "a/b"),
                ("BUILD_NUMBER", "9"),
            ]),
        );
        assert!(label.starts_with("a-b-9-"), "{label}");
    }

    #[test]
    fn a_literal_is_used_as_given() {
        let (label, warning) = resolve(
            &ScopeConfig::Literal("release".into()),
            &lookup(&[("GITHUB_ACTIONS", "true"), ("GITHUB_HEAD_REF", "x")]),
        );
        assert_eq!((label.as_str(), warning), ("release", None));
    }
}
