use std::path::Path;

use omni_generator_configurations::{
    JsRuntimeOption, RunJavaScriptActionConfiguration,
};

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        run_custom_commons::{run_custom_commons, target_path},
    },
    error::Error,
};

pub async fn run_javascript<'a>(
    config: &RunJavaScriptActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let script_path = ctx.generator_dir.join(&config.script);
    let tp = target_path(&config.common, ctx, sys).await?;
    let relative_path = pathdiff::diff_paths(&script_path, &tp);
    let command = build_cmd(
        config.runtime,
        relative_path.as_deref().unwrap_or(&script_path),
    );

    run_custom_commons(&command, Some(tp.as_path()), &config.common, ctx, sys)
        .await
}

fn auto_detect_runtime_option() -> Option<JsRuntimeOption> {
    Some(if which::which("bun").is_ok() {
        JsRuntimeOption::Bun
    } else if which::which("deno").is_ok() {
        JsRuntimeOption::Deno
    } else if which::which("node").is_ok() {
        JsRuntimeOption::Node
    } else {
        return None;
    })
}

fn build_cmd(runtime: JsRuntimeOption, main_module: &Path) -> String {
    let rt = if runtime == JsRuntimeOption::Auto {
        auto_detect_runtime_option().expect("Can't auto detect runtime")
    } else {
        runtime
    };

    let mut cmd_builder = String::new();

    let cmd = match rt {
        JsRuntimeOption::Deno => "deno",
        JsRuntimeOption::Node => "node",
        JsRuntimeOption::Bun => "bun",
        JsRuntimeOption::Auto => {
            unreachable!("Auto select runtime should be unreachable")
        }
    };

    cmd_builder.push_str(cmd);

    if rt != JsRuntimeOption::Node {
        cmd_builder.push_str(" run");
    }

    if rt == JsRuntimeOption::Deno {
        cmd_builder.push_str(" --allow-all");
    }

    cmd_builder.push(' ');
    cmd_builder.push_str(&main_module.to_string_lossy());

    cmd_builder
}
