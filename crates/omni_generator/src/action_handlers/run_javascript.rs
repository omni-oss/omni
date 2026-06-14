use js_runtime::impls::DelegatingJsRuntimeOption;
use omni_generator_configurations::{
    JsRuntimeOption, RunJavaScriptActionConfiguration,
};

use crate::{
    GeneratorSys, ScriptInvocation, ScriptParams,
    action_handlers::HandlerContext, error::Error, utils::expand_json_value,
};

/// Executes a `run-javascript` action by handing the script off to the shared,
/// lazily-spawned generator script runner.
///
/// All scripts in a run (including those reached through nested
/// `run-generator` actions) share a runner per JS runtime, and their
/// file-system side effects flow through the same transactional overlay as the
/// rest of the generator (see [`crate::LazyScriptRunner`]).
///
/// The configured `data` is rendered against the generator's template context
/// (via [`expand_json_value`]) and handed to the script alongside the current
/// run's `dry_run` flag. The action's `runtime` selects which JS runtime runs
/// it.
pub async fn run_javascript<'a>(
    config: &RunJavaScriptActionConfiguration,
    ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let script_path = ctx.generator_dir.join(&config.script);

    let data =
        expand_json_value(ctx.tera_context_values, None, "data", &config.data)?
            .into_owned();

    let invocation = ScriptInvocation {
        path: script_path.to_string_lossy().into_owned(),
        params: ScriptParams {
            dry_run: ctx.dry_run,
            data,
            output_dir: ctx.output_dir.to_string_lossy().into_owned(),
        },
    };

    ctx.script_runner
        .run_scripts(map_runtime(config.runtime), &[invocation])
        .await
}

/// Maps the configuration's runtime option to the runner's runtime option.
fn map_runtime(runtime: JsRuntimeOption) -> DelegatingJsRuntimeOption {
    match runtime {
        JsRuntimeOption::Deno => DelegatingJsRuntimeOption::Deno,
        JsRuntimeOption::Node => DelegatingJsRuntimeOption::Node,
        JsRuntimeOption::Bun => DelegatingJsRuntimeOption::Bun,
        JsRuntimeOption::Auto => DelegatingJsRuntimeOption::Auto,
    }
}
