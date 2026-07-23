use std::{borrow::Cow, path::Path};

use maps::{Map, UnorderedMap};
use omni_capabilities::CapabilityRules;
use omni_generator_configurations::{
    ActionConfiguration, CapabilitiesStrictness, Generator,
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_messages::{
    GeneratorEventSubscriber, NoopSubscriber,
    generator::events::{
        GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
        GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
    },
};
use omni_utils::path::clean;
use strum::IntoDiscriminant;
use value_bag::OwnedValueBag;

use crate::{
    GeneratorSysFull, JsScriptRunner,
    action_handlers::{
        HandlerContext, add, add_content, add_many, append, append_content,
        modify, modify_content, prepend, prepend_content, run_command,
        run_generator, run_javascript, transform, transform_many,
    },
    error::{Error, ErrorInner},
    gen_session::GenSession,
    utils::get_tera_context,
};

#[derive(Debug)]
pub struct ExecuteActionsArgs<'a, S: GeneratorEventSubscriber = NoopSubscriber>
{
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub generator_dir: &'a Path,
    pub workspace_dir: &'a Path,
    pub generator_name: &'a str,
    pub scope_id: Option<&'a str>,
    pub current_dir: &'a Path,
    pub actions: &'a [ActionConfiguration],
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub targets: &'a UnorderedMap<String, OmniPath>,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub overwrite: Option<OverwriteConfiguration>,
    pub available_generators: &'a [Cow<'a, GeneratorConfiguration>],
    pub env: &'a Map<String, String>,
    /// The workspace-level capability floor, forwarded as the workspace floor to
    /// nested generators and the outermost level of the inherited ceiling.
    pub workspace_capabilities: &'a CapabilityRules<Generator>,
    /// The inherited capability ceiling (ordered, outermost first) that this
    /// generator may only narrow. See [`HandlerContext`].
    pub inherited_capabilities: &'a [CapabilityRules<Generator>],
    /// The current generator's capability policy, threaded to `run-javascript`.
    pub capabilities: &'a CapabilityRules<Generator>,
    /// Effective floor-gap strictness for this generator: the most-severe of the
    /// workspace, all ancestor generators, and this generator's own stance.
    pub capabilities_strictness: CapabilitiesStrictness,
    pub use_input_defaults: bool,
    pub js_script_runner: &'a dyn JsScriptRunner,
    pub input_provider: &'a dyn omni_input_provider::InputProvider<
        omni_generator_configurations::Generator,
    >,
    pub subscriber: &'a S,
    /// Current `run-generator` nesting depth of the generator being executed.
    pub depth: usize,
    /// Maximum allowed nesting depth, propagated to nested runs.
    pub max_depth: usize,
}

pub async fn execute_actions<'a, S: GeneratorEventSubscriber>(
    args: &ExecuteActionsArgs<'a, S>,
    gen_session: &GenSession,
    sys: &impl GeneratorSysFull,
) -> Result<(), Error> {
    let mut tera_context = get_tera_context(args.context_values);

    let output_dir = args.current_dir.join(args.output_dir);
    let expanded_output = omni_tera::one_off(
        &output_dir.to_string_lossy(),
        "output_dir",
        &tera_context,
    )?;
    let output_path = Path::new(&expanded_output);

    if let Some(diff) =
        pathdiff::diff_paths(&output_path, clean(&args.workspace_dir))
    {
        tera_context.insert("output_dir", &diff);
    } else {
        tera_context.insert("output_dir", &output_path);
    }

    for (index, action) in args.actions.iter().enumerate() {
        let action_name = get_action_name(index, action, &tera_context)?;

        if skip(&action_name, action, &tera_context)? {
            args.subscriber
                .on_action_skipped(GeneratorActionSkippedEvent {
                    name: action_name.clone(),
                    reason: Some("condition not met".to_string()),
                    depth: args.depth,
                })
                .await;
            continue;
        }

        let handler_context = HandlerContext {
            context_values: args.context_values,
            tera_context_values: &tera_context,
            dry_run: args.dry_run,
            output_dir: output_path,
            generator_targets: args.targets,
            target_overrides: args.target_overrides,
            scope_id: args.scope_id,
            generator_name: args.generator_name,
            generator_dir: args.generator_dir,
            overwrite: args.overwrite,
            available_generators: args.available_generators,
            workspace_dir: args.workspace_dir,
            resolved_action_name: action_name.as_str(),
            current_dir: args.current_dir,
            env: args.env,
            workspace_capabilities: args.workspace_capabilities,
            inherited_capabilities: args.inherited_capabilities,
            capabilities: args.capabilities,
            capabilities_strictness: args.capabilities_strictness,
            gen_session,
            js_script_runner: args.js_script_runner,
            input_provider: args.input_provider,
            subscriber: args.subscriber,
            use_input_defaults: args.use_input_defaults,
            depth: args.depth,
            max_depth: args.max_depth,
        };

        let in_progress_message =
            get_in_progress_message(&action_name, action, &tera_context)?;

        args.subscriber
            .on_action_in_progress(GeneratorActionInProgressEvent {
                name: action_name.clone(),
                message: in_progress_message,
                depth: args.depth,
            })
            .await;

        let result = match action {
            ActionConfiguration::Add { action } => {
                add(action, &handler_context, sys).await
            }
            ActionConfiguration::AddContent { action } => {
                add_content(action, &handler_context, sys).await
            }
            ActionConfiguration::AddMany { action } => {
                add_many(action, &handler_context, sys).await
            }
            ActionConfiguration::RunGenerator { action } => {
                run_generator(action, &handler_context, sys).await
            }
            ActionConfiguration::Modify { action } => {
                modify(action, &handler_context, sys).await
            }
            ActionConfiguration::Append { action } => {
                append(action, &handler_context, sys).await
            }
            ActionConfiguration::ModifyContent { action } => {
                modify_content(action, &handler_context, sys).await
            }
            ActionConfiguration::AppendContent { action } => {
                append_content(action, &handler_context, sys).await
            }
            ActionConfiguration::Prepend { action } => {
                prepend(action, &handler_context, sys).await
            }
            ActionConfiguration::PrependContent { action } => {
                prepend_content(action, &handler_context, sys).await
            }
            ActionConfiguration::Transform { action } => {
                transform(action, &handler_context, sys).await
            }
            ActionConfiguration::TransformMany { action } => {
                transform_many(action, &handler_context, sys).await
            }
            ActionConfiguration::RunCommand { action } => {
                run_command(action, &handler_context, sys).await
            }
            ActionConfiguration::RunJavaScript { action } => {
                run_javascript(action, &handler_context, sys).await
            }
        };

        if let Err(e) = result {
            let error_message =
                get_error_message(&action_name, &e, action, &tera_context)?;

            args.subscriber
                .on_action_failed(GeneratorActionFailedEvent {
                    name: action_name.clone(),
                    message: error_message,
                    depth: args.depth,
                })
                .await;

            return Err(e);
        } else {
            let success_message =
                get_success_message(&action_name, action, &tera_context)?;

            args.subscriber
                .on_action_success(GeneratorActionSuccessEvent {
                    name: action_name.clone(),
                    message: success_message,
                    depth: args.depth,
                })
                .await;
        }
    }

    Ok(())
}

fn skip(
    name: &str,
    action: &ActionConfiguration,
    tera_context: &omni_tera::Context,
) -> Result<bool, Error> {
    let if_expr = action.base().r#if.as_deref();

    if let Some(if_expr) = if_expr {
        let result = omni_tera::one_off(
            if_expr,
            &format!("if condition for action {}: {}", name, if_expr),
            tera_context,
        )?;
        let result = result.trim();
        validate_bool_result(result, if_expr, "if")?;

        Ok(result == "false")
    } else {
        Ok(false)
    }
}

fn validate_bool_result(
    result: &str,
    expr: &str,
    expr_name: &str,
) -> Result<(), Error> {
    if result == "true" || result == "false" {
        Ok(())
    } else {
        Err(ErrorInner::InvalidBooleanResult {
            result: result.to_string(),
            expr: expr.to_string(),
            expr_name: expr_name.to_string(),
        })?
    }
}

fn render_text(
    message: &str,
    name: &str,
    tera_context: &omni_tera::Context,
) -> Result<String, Error> {
    let result = omni_tera::one_off(message, name, tera_context)?;

    Ok(result)
}

fn get_error_message(
    action_name: &str,
    error: &Error,
    action: &ActionConfiguration,
    tera_context: &omni_tera::Context,
) -> Result<String, Error> {
    let message = action.base().error_message.as_deref();

    if let Some(message) = message {
        let mut error_ctx = tera_context.clone();
        error_ctx.insert("error", &error.to_string());

        render_text(
            message,
            &format!("error_message for action {}", action_name),
            &error_ctx,
        )
    } else {
        Ok(format!(
            "Encountered an error while executing action: {}",
            error.to_string()
        ))
    }
}

fn get_in_progress_message(
    action_name: &str,
    action: &ActionConfiguration,
    tera_context: &omni_tera::Context,
) -> Result<String, Error> {
    let message = action.base().in_progress_message.as_deref();

    if let Some(message) = message {
        render_text(
            message,
            &format!("in_progress_message for action {}", action_name),
            tera_context,
        )
    } else {
        Ok("Executing action...".to_string())
    }
}

fn get_success_message(
    action_name: &str,
    action: &ActionConfiguration,
    tera_context: &omni_tera::Context,
) -> Result<String, Error> {
    let message = action.base().success_message.as_deref();

    if let Some(message) = message {
        render_text(
            message,
            &format!("success_message for action {}", action_name),
            tera_context,
        )
    } else {
        Ok("Executed action successfully".to_string())
    }
}

fn get_action_name(
    index: usize,
    action: &ActionConfiguration,
    tera_context: &omni_tera::Context,
) -> Result<String, Error> {
    let name = action.base().name.as_deref();

    if let Some(name) = name {
        render_text(name, &format!("name for action#{}", index), tera_context)
    } else {
        let action_type = action.discriminant();
        Ok(format!("#{}-{}", index + 1, action_type))
    }
}
