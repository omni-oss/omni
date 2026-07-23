use bridge_rpc_runner::DelegatingJsRuntimeOption;
use omni_capabilities::{CapabilityRules, PathRoots, Root};
use omni_generator_configurations::{
    Generator, GeneratorContext, JsRuntimeOption,
    RunJavaScriptActionConfiguration,
};
use omni_messages::{GeneratorEventSubscriber, publish::diagnostic};

use crate::{
    EffectivePolicy, GeneratorSys, ScriptInvocation, ScriptParams,
    action_handlers::HandlerContext, error::Error, utils::expand_json_value,
};

pub async fn run_javascript<'a, S: GeneratorEventSubscriber>(
    config: &RunJavaScriptActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    // `canonicalize` (used when the context is loaded) returns paths with the
    // Windows verbatim prefix (`\\?\`).  `omni_utils::path::clean` strips it
    // so the JS bridge receives a plain path that `pathToFileURL` can convert
    // to a valid `file://` URL.
    let script_path =
        omni_utils::path::clean(ctx.generator_dir.join(&config.script));
    let output_dir = omni_utils::path::clean(&ctx.output_dir);

    let data =
        expand_json_value(ctx.tera_context_values, None, "data", &config.data)?
            .into_owned();

    let invocation = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: ctx.dry_run,
            data,
            output_dir: output_dir.to_string_lossy().into_owned(),
        },
    };

    // The effective policy for this script is a stack of **distinct** levels,
    // ordered outermost → innermost:
    //
    //   inherited ceiling (workspace ⏺ ancestor generators)
    //     ⏺ this generator's own policy
    //     ⏺ this action's policy
    //
    // Authorization applies the shrink-only (attenuation) model over these
    // levels: each level can only *narrow* the authority it inherited, so a
    // deeper level can never grant itself access an ancestor did not, and a
    // `deny` at any level wins. The levels are kept separate (not pre-merged) so
    // the intersection is exact per operation.
    let mut levels: Vec<CapabilityRules<Generator>> =
        ctx.inherited_capabilities.to_vec();
    levels.push(ctx.capabilities.clone());
    levels.push(config.capabilities.rules.clone());

    let policy = EffectivePolicy {
        levels,
        roots: PathRoots::new()
            .with(Root::Workspace, ctx.workspace_dir)
            .with(Root::Project, ctx.output_dir),
        context: GeneratorContext {
            action: Some(ctx.resolved_action_name.to_string()),
            target: None,
        },
        // The effective stance is the most-severe of everything up to and
        // including this generator (already accumulated in
        // `ctx.capabilities_strictness`) and this action's own stance.
        strictness: ctx
            .capabilities_strictness
            .max(config.capabilities.strictness),
    };

    let result = ctx
        .js_script_runner
        .run_scripts(map_runtime(config.runtime), &policy, &[invocation])
        .await?;

    // Surface each structured diagnostic (e.g. capability `on_unenforceable:
    // warn` gaps) through the run's diagnostic subscriber, at its own level, so
    // the choice is visible rather than silent.
    for d in result.diagnostics {
        diagnostic!(ctx.subscriber, d.level, "{}", d.message).await;
    }

    Ok(())
}

fn map_runtime(runtime: JsRuntimeOption) -> DelegatingJsRuntimeOption {
    match runtime {
        JsRuntimeOption::Deno => DelegatingJsRuntimeOption::Deno,
        JsRuntimeOption::Node => DelegatingJsRuntimeOption::Node,
        JsRuntimeOption::Bun => DelegatingJsRuntimeOption::Bun,
        JsRuntimeOption::Auto => DelegatingJsRuntimeOption::Auto,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bridge_rpc_runner::DelegatingJsRuntimeOption;
    use omni_generator_configurations::{
        BaseActionConfiguration, JsRuntimeOption,
        RunJavaScriptActionConfiguration,
    };
    use system_traits::impls::RealSys;

    use super::super::test_harness::{Fixture, MockJsScriptRunner};
    use super::{map_runtime, run_javascript};

    fn config(
        script: &str,
        runtime: JsRuntimeOption,
    ) -> RunJavaScriptActionConfiguration {
        RunJavaScriptActionConfiguration {
            base: BaseActionConfiguration {
                r#if: None,
                name: None,
                in_progress_message: None,
                success_message: None,
                error_message: None,
            },
            data: Default::default(),
            runtime,
            script: script.into(),
            capabilities: Default::default(),
        }
    }

    #[tokio::test]
    async fn dispatches_invocation_to_runner() {
        let mock = MockJsScriptRunner::default();
        let mock_ref = mock.clone();
        let fix = Fixture::new().with_js_script_runner(Box::new(mock));
        let ctx = fix.ctx();
        let sys = RealSys;

        run_javascript(&config("gen.js", JsRuntimeOption::Auto), &ctx, &sys)
            .await
            .unwrap();

        let invs = mock_ref.invocations.lock().unwrap();
        assert_eq!(invs.len(), 1);
        let (_, scripts) = &invs[0];
        assert_eq!(scripts.len(), 1);
        let expected_path =
            omni_utils::path::clean(fix.generator.path().join("gen.js"));
        let script0_path = omni_utils::path::clean(Path::new(&scripts[0].path));
        assert_eq!(script0_path, expected_path);
        assert!(!scripts[0].params.dry_run);
    }

    #[test]
    fn map_runtime_deno() {
        assert!(matches!(
            map_runtime(JsRuntimeOption::Deno),
            DelegatingJsRuntimeOption::Deno
        ));
    }

    #[test]
    fn map_runtime_node() {
        assert!(matches!(
            map_runtime(JsRuntimeOption::Node),
            DelegatingJsRuntimeOption::Node
        ));
    }

    #[test]
    fn map_runtime_bun() {
        assert!(matches!(
            map_runtime(JsRuntimeOption::Bun),
            DelegatingJsRuntimeOption::Bun
        ));
    }

    #[test]
    fn map_runtime_auto() {
        assert!(matches!(
            map_runtime(JsRuntimeOption::Auto),
            DelegatingJsRuntimeOption::Auto
        ));
    }
}
