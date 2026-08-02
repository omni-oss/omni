use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use omni_generator_configurations::{
    ActionConfiguration, GeneratorConfiguration,
};
use sets::unordered_set;

use crate::error::{Error, ErrorInner};

pub fn validate(
    generators: &[Cow<GeneratorConfiguration>],
) -> Result<(), Error> {
    let mut names = unordered_set!();

    for generator in generators {
        if names.contains(&generator.name) {
            return Err(ErrorInner::new_duplicate_generator_name(
                generator.name.clone(),
                generator.config_path.clone(),
            ))?;
        }

        names.insert(generator.name.clone());

        // Fail-closed authoring: reject any capability rule whose domain the
        // generator subsystem does not support (e.g. `net`) before a run can
        // begin.
        omni_capabilities::validate(&generator.capabilities.rules).map_err(
            |source| {
                ErrorInner::new_invalid_capabilities(
                    generator.name.clone(),
                    generator.config_path.clone(),
                    source,
                )
            },
        )?;

        // Action-level capabilities (on `run-javascript`) are the last cascade
        // level; validate them here too so an unsupported domain fails at load
        // rather than mid-run.
        for action in &generator.actions {
            if let ActionConfiguration::RunJavaScript { action: js } = action {
                omni_capabilities::validate(&js.capabilities.rules).map_err(
                    |source| {
                        ErrorInner::new_invalid_capabilities(
                            generator.name.clone(),
                            generator.config_path.clone(),
                            source,
                        )
                    },
                )?;
            }
        }
    }

    Ok(())
}

/// Detects if running `entry_generator` would result in a recursive call
/// cycle — i.e., if the generator can reach itself (directly or indirectly)
/// through `RunGenerator` actions.
pub fn detect_recursion(
    entry_generator: &GeneratorConfiguration,
    generators: &[Cow<GeneratorConfiguration>],
) -> Result<(), Error> {
    let generator_map: HashMap<&str, &GeneratorConfiguration> = generators
        .iter()
        .map(|g| (g.name.as_str(), g.as_ref()))
        .collect();

    let mut visited = HashSet::new();
    let mut path = HashSet::new();

    dfs(entry_generator, &generator_map, &mut visited, &mut path)
}

/// DFS over the `RunGenerator` call graph.
///
/// `visited` tracks globally explored nodes to avoid redundant work.
/// `path` tracks the nodes on the current DFS stack to detect back-edges
/// (cycles). When a node is encountered that is already in `path`, a cycle
/// is present.
fn dfs<'a>(
    generator: &'a GeneratorConfiguration,
    generator_map: &HashMap<&str, &'a GeneratorConfiguration>,
    visited: &mut HashSet<&'a str>,
    path: &mut HashSet<&'a str>,
) -> Result<(), Error> {
    if path.contains(generator.name.as_str()) {
        return Err(ErrorInner::new_generator_recursion(
            generator.name.clone(),
            generator.config_path.clone(),
        )
        .into());
    }

    if visited.contains(generator.name.as_str()) {
        return Ok(());
    }

    visited.insert(generator.name.as_str());
    path.insert(generator.name.as_str());

    for action in &generator.actions {
        if let ActionConfiguration::RunGenerator { action: run } = action {
            if let Some(called) = generator_map.get(run.generator.as_str()) {
                dfs(*called, generator_map, visited, path)?;
            }
        }
    }

    path.remove(generator.name.as_str());

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;

    use maps::UnorderedMap;
    use omni_generator_configurations::{
        ActionConfiguration, BaseActionConfiguration, GeneratorConfiguration,
        InputValuesConfiguration, RunGeneratorActionConfiguration,
    };

    use super::{detect_recursion, validate};
    use crate::error::ErrorKind;

    fn make_gen(name: &str, calls: &[&str]) -> GeneratorConfiguration {
        GeneratorConfiguration {
            config_path: PathBuf::from(format!("/fake/{name}.yaml")),
            scope_id: None,
            user_invocable: true,
            name: name.to_string(),
            display_name: None,
            description: None,
            inputs: vec![],
            actions: calls
                .iter()
                .map(|target| ActionConfiguration::RunGenerator {
                    action: RunGeneratorActionConfiguration {
                        base: BaseActionConfiguration {
                            r#if: None,
                            name: None,
                            in_progress_message: None,
                            success_message: None,
                            error_message: None,
                        },
                        generator: target.to_string(),
                        input_values: InputValuesConfiguration::default(),
                        args: UnorderedMap::default(),
                        output_dir: None,
                        targets: UnorderedMap::default(),
                    },
                })
                .collect(),
            vars: UnorderedMap::default(),
            targets: UnorderedMap::default(),
            capabilities: Default::default(),
        }
    }

    fn to_cows(
        gens: &'_ [GeneratorConfiguration],
    ) -> Vec<Cow<'_, GeneratorConfiguration>> {
        gens.iter().map(Cow::Borrowed).collect()
    }

    // --- validate ---

    #[test]
    fn validate_empty_list_is_ok() {
        assert!(validate(&[]).is_ok());
    }

    #[test]
    fn validate_unique_names_is_ok() {
        let a = make_gen("a", &[]);
        let b = make_gen("b", &[]);
        let c = make_gen("c", &[]);
        assert!(validate(&to_cows(&[a, b, c])).is_ok());
    }

    #[test]
    fn accepts_generator_level_env_capability() {
        // Generators now govern `env` (it is in `Generator::SUPPORTED`), so an
        // `env` rule must load without error.
        let mut cfg = make_gen("g", &[]);
        cfg.capabilities.rules = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["HOME"] }]"#,
        )
        .expect("parses as a raw capabilities array");
        validate(&to_cows(std::slice::from_ref(&cfg)))
            .expect("env is a supported generator domain");
    }

    #[test]
    fn accepts_action_level_env_capability() {
        use omni_generator_configurations::RunJavaScriptActionConfiguration;

        // An action-level `env` rule must also load without error.
        let mut cfg = make_gen("g", &[]);
        let mut js = RunJavaScriptActionConfiguration {
            base: BaseActionConfiguration {
                r#if: None,
                name: None,
                in_progress_message: None,
                success_message: None,
                error_message: None,
            },
            data: Default::default(),
            runtime: Default::default(),
            script: PathBuf::from("gen.js"),
            capabilities: Default::default(),
        };
        js.capabilities.rules = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["HOME"] }]"#,
        )
        .expect("parses as a raw capabilities array");
        cfg.actions = vec![ActionConfiguration::RunJavaScript { action: js }];

        validate(&to_cows(std::slice::from_ref(&cfg)))
            .expect("env is a supported generator-action domain");
    }

    #[test]
    fn allows_capability_with_supported_domain() {
        let mut cfg = make_gen("g", &[]);
        cfg.capabilities.rules = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        )
        .expect("parses");
        assert!(validate(&to_cows(std::slice::from_ref(&cfg))).is_ok());
    }

    #[test]
    fn validate_duplicate_name_returns_error() {
        let a1 = make_gen("foo", &[]);
        let a2 = make_gen("foo", &[]);
        let err = validate(&to_cows(&[a1, a2])).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DuplicateGeneratorName);
    }

    #[test]
    fn validate_duplicate_name_error_reports_the_name() {
        let a1 = make_gen("my-gen", &[]);
        let a2 = make_gen("my-gen", &[]);
        let err = validate(&to_cows(&[a1, a2])).unwrap_err();
        assert!(err.to_string().contains("my-gen"));
    }

    // --- detect_recursion ---

    #[test]
    fn no_actions_is_ok() {
        let a = make_gen("a", &[]);
        let arr = [a.clone()];
        let gens = to_cows(&arr);
        assert!(detect_recursion(&a, &gens).is_ok());
    }

    #[test]
    fn linear_chain_without_cycle_is_ok() {
        // a → b → c (no cycle)
        let a = make_gen("a", &["b"]);
        let b = make_gen("b", &["c"]);
        let c = make_gen("c", &[]);
        let arr = [a.clone(), b, c];
        let gens = to_cows(&arr);
        assert!(detect_recursion(&a, &gens).is_ok());
    }

    #[test]
    fn direct_self_call_is_recursion() {
        // a → a
        let a = make_gen("a", &["a"]);
        let arr = [a.clone()];
        let gens = to_cows(&arr);
        let err = detect_recursion(&a, &gens).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::GeneratorRecursion);
    }

    #[test]
    fn indirect_cycle_is_recursion() {
        // a → b → a
        let a = make_gen("a", &["b"]);
        let b = make_gen("b", &["a"]);
        let arr = [a.clone(), b];
        let gens = to_cows(&arr);
        let err = detect_recursion(&a, &gens).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::GeneratorRecursion);
    }

    #[test]
    fn longer_cycle_is_recursion() {
        // a → b → c → a
        let a = make_gen("a", &["b"]);
        let b = make_gen("b", &["c"]);
        let c = make_gen("c", &["a"]);
        let arr = [a.clone(), b, c];
        let gens = to_cows(&arr);
        let err = detect_recursion(&a, &gens).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::GeneratorRecursion);
    }

    #[test]
    fn diamond_without_cycle_is_ok() {
        // a → b, a → c, b → d, c → d (shared leaf, not a cycle)
        let a = make_gen("a", &["b", "c"]);
        let b = make_gen("b", &["d"]);
        let c = make_gen("c", &["d"]);
        let d = make_gen("d", &[]);
        let arr = [a.clone(), b, c, d];
        let gens = to_cows(&arr);
        assert!(detect_recursion(&a, &gens).is_ok());
    }

    #[test]
    fn reference_to_unknown_generator_is_ok() {
        // unknown generators (not in the list) are simply skipped
        let a = make_gen("a", &["nonexistent"]);
        let arr = [a.clone()];
        let gens = to_cows(&arr);
        assert!(detect_recursion(&a, &gens).is_ok());
    }

    #[test]
    fn cycle_in_non_entry_subgraph_is_still_detected() {
        // a → b → c → b (cycle in b/c, reachable from a)
        let a = make_gen("a", &["b"]);
        let b = make_gen("b", &["c"]);
        let c = make_gen("c", &["b"]);
        let arr = [a.clone(), b, c];
        let gens = to_cows(&arr);
        let err = detect_recursion(&a, &gens).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::GeneratorRecursion);
    }

    #[test]
    fn recursion_error_reports_generator_name() {
        let a = make_gen("looping", &["looping"]);
        let arr = [a.clone()];
        let gens = to_cows(&arr);
        let err = detect_recursion(&a, &gens).unwrap_err();
        assert!(err.to_string().contains("looping"));
    }
}
