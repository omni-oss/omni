use js_runtime::impls::DelegatingJsRuntimeOption;
use omni_generator_configurations::{
    JsRuntimeOption, RunJavaScriptActionConfiguration,
};

use crate::{
    GeneratorSys, ScriptInvocation, ScriptParams,
    action_handlers::HandlerContext, error::Error, utils::expand_json_value,
};

pub async fn run_javascript<'a>(
    config: &RunJavaScriptActionConfiguration,
    ctx: &HandlerContext<'a>,
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

    ctx.js_script_runner
        .run_scripts(map_runtime(config.runtime), &[invocation])
        .await
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
    use js_runtime::impls::DelegatingJsRuntimeOption;
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
        let expected_path = fix.generator.path().join("gen.js");
        assert_eq!(scripts[0].path, expected_path.to_string_lossy().as_ref());
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
