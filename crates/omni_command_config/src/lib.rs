mod command_config;
mod resolve;

pub use command_config::*;
pub use resolve::*;

#[cfg(all(test, feature = "serde"))]
mod tests {
    use maps::Map;
    use omni_tera::Context;

    use super::*;

    fn from_json(json: &str) -> CommandConfig {
        serde_json::from_str(json).expect("should deserialize")
    }

    #[test]
    fn string_deserializes_to_shell() {
        let cmd: CommandConfig = from_json(r#""vite dev""#);
        assert_eq!(cmd, CommandConfig::Shell("vite dev".to_string()));
    }

    #[test]
    fn sequence_deserializes_to_argv() {
        let cmd: CommandConfig = from_json(r#"["bun", "exec", "cp -r a b"]"#);
        assert_eq!(
            cmd,
            CommandConfig::Argv(vec![
                "bun".to_string(),
                "exec".to_string(),
                "cp -r a b".to_string(),
            ])
        );
    }

    #[test]
    fn invalid_tera_rejected_in_both_forms() {
        assert!(serde_json::from_str::<CommandConfig>(r#""{{ bad""#).is_err());
        assert!(
            serde_json::from_str::<CommandConfig>(r#"["echo", "{{ bad"]"#)
                .is_err()
        );
    }

    #[test]
    fn resolve_shell_single_quotes_uniform() {
        // Single-quoted section stays a single argument on every platform.
        let cmd = CommandConfig::Shell(
            "bun exec 'cp -r ../a/dist ./dist'".to_string(),
        );
        let argv = resolve_command(&cmd, None, None).unwrap();
        assert_eq!(
            argv.to_argv(),
            vec!["bun", "exec", "cp -r ../a/dist ./dist"]
        );
    }

    #[test]
    fn resolve_shell_env_expansion() {
        let cmd = CommandConfig::Shell("echo $GREETING".to_string());
        let mut env = Map::default();
        env.insert("GREETING".to_string(), "hello".to_string());
        let argv = resolve_command(&cmd, None, Some(&env)).unwrap();
        assert_eq!(argv.to_argv(), vec!["echo", "hello"]);
    }

    #[test]
    fn resolve_shell_tera_expansion() {
        let cmd = CommandConfig::Shell("echo {{ name }}".to_string());
        let mut ctx = Context::new();
        ctx.insert("name", "world");
        let argv = resolve_command(&cmd, Some(&ctx), None).unwrap();
        assert_eq!(argv.to_argv(), vec!["echo", "world"]);
    }

    #[test]
    fn resolve_shell_none_contexts_skip_steps() {
        let cmd = CommandConfig::Shell("echo $NOPE {{ nope }}".to_string());
        let argv = resolve_command(&cmd, None, None).unwrap();
        // No env-expand and no Tera render: literal passthrough (shlex split).
        assert_eq!(argv.to_argv(), vec!["echo", "$NOPE", "{{", "nope", "}}"]);
    }

    #[test]
    fn resolve_argv_element_with_spaces_stays_one_arg() {
        let cmd = CommandConfig::Argv(vec![
            "bun".to_string(),
            "exec".to_string(),
            "cp -r ../a/dist ./dist".to_string(),
        ]);
        let argv = resolve_command(&cmd, None, None).unwrap();
        assert_eq!(
            argv.to_argv(),
            vec!["bun", "exec", "cp -r ../a/dist ./dist"]
        );
    }

    #[test]
    fn resolve_argv_env_expansion_per_element() {
        let cmd = CommandConfig::Argv(vec![
            "echo".to_string(),
            "$GREETING world".to_string(),
        ]);
        let mut env = Map::default();
        env.insert("GREETING".to_string(), "hello".to_string());
        let argv = resolve_command(&cmd, None, Some(&env)).unwrap();
        // Env-expanded, but not split: still one argument.
        assert_eq!(argv.to_argv(), vec!["echo", "hello world"]);
    }

    #[test]
    fn resolve_argv_tera_expansion_no_split() {
        let cmd = CommandConfig::Argv(vec![
            "echo".to_string(),
            "{{ msg }}".to_string(),
        ]);
        let mut ctx = Context::new();
        ctx.insert("msg", "a b c");
        let argv = resolve_command(&cmd, Some(&ctx), None).unwrap();
        assert_eq!(argv.to_argv(), vec!["echo", "a b c"]);
    }

    #[test]
    fn canonical_shell_is_raw_and_borrowed() {
        let cmd = CommandConfig::Shell("echo {{ name }}".to_string());
        let canonical = cmd.canonical();
        assert!(matches!(canonical, std::borrow::Cow::Borrowed(_)));
        assert_eq!(canonical, "echo {{ name }}");
    }

    #[test]
    fn canonical_argv_is_deterministic_json() {
        let cmd =
            CommandConfig::Argv(vec!["echo".to_string(), "a b".to_string()]);
        let canonical = cmd.canonical();
        assert!(matches!(canonical, std::borrow::Cow::Owned(_)));
        assert_eq!(canonical, r#"["echo","a b"]"#);
    }

    #[test]
    fn no_template_fast_path_bypasses_tera() {
        // A malformed-but-marker-free string would only error if Tera ran.
        let cmd = CommandConfig::Shell("echo hello".to_string());
        let ctx = Context::new();
        let argv = resolve_command(&cmd, Some(&ctx), None).unwrap();
        assert_eq!(argv.to_argv(), vec!["echo", "hello"]);
    }

    #[test]
    fn template_markers_still_render() {
        let cmd = CommandConfig::Shell("echo {{ name }}".to_string());
        let mut ctx = Context::new();
        ctx.insert("name", "rendered");
        let argv = resolve_command(&cmd, Some(&ctx), None).unwrap();
        assert_eq!(argv.to_argv(), vec!["echo", "rendered"]);
    }
}
