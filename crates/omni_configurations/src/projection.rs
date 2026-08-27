pub use omni_projection_configurations::*;

use crate::SourceConfig;

pub type ProjectionSourceConfiguration = SourceConfig<ProjectionExtra>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkspaceConfiguration;

    fn parse_source(
        json: &str,
    ) -> Result<ProjectionSourceConfiguration, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn parse_workspace(
        json: &str,
    ) -> Result<WorkspaceConfiguration, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn parses_git_and_local_projection_sources() {
        let git = parse_source(
            r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"team-ai-skills","projections":[{"strategy":"namespaced"}]}"#,
        )
        .expect("valid git projection source");
        match git {
            SourceConfig::Git(g) => {
                assert_eq!(g.extra.id, "team-ai-skills");
                assert_eq!(g.extra.projections.len(), 1);
            }
            SourceConfig::Local(_) => panic!("expected git"),
        }

        let local = parse_source(
            r#"{"source":"local","path":"./vendor","id":"shared_scripts","projections":[{"strategy":"flatten","rules":[{"match":"**/*.sh"}]}]}"#,
        )
        .expect("valid local projection source");
        match local {
            SourceConfig::Local(l) => {
                assert_eq!(l.extra.id, "shared_scripts");
            }
            SourceConfig::Git(_) => panic!("expected local"),
        }
    }

    #[test]
    fn projection_source_rejects_unknown_key() {
        assert!(
            parse_source(
                r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","id":"a","projections":[],"typo":1}"#,
            )
            .is_err(),
            "unknown key under a projection source must be rejected"
        );
    }

    #[test]
    fn workspace_rejects_duplicate_projection_id() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"projections":[{"source":"local","path":"./a","id":"dup","projections":[{"strategy":"namespaced"}]},{"source":"local","path":"./b","id":"dup","projections":[{"strategy":"namespaced"}]}]}"#,
            )
            .is_err(),
            "duplicate projection id must be rejected"
        );
    }

    #[test]
    fn workspace_rejects_rules_on_namespaced_strategy() {
        assert!(
            parse_workspace(
                r#"{"projects":[],"projections":[{"source":"local","path":"./a","id":"a","projections":[{"strategy":"namespaced","rules":[{"match":"**/*"}]}]}]}"#,
            )
            .is_err(),
            "`rules` on a namespaced strategy must be rejected"
        );
    }
}
