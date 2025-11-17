use std::path::Path;

use derive_new::new;
use maps::UnorderedMap;
use omni_generator_configurations::{
    ActionConfiguration, GeneratorConfiguration, OmniPath,
    OverwriteConfiguration,
};
use strum::IntoDiscriminant;
use value_bag::OwnedValueBag;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext, add, add_content, add_many, append, append_content,
        modify, modify_content, prepend, prepend_content, run_generator,
    },
    error::{Error, ErrorInner},
    utils::get_tera_context,
};

#[derive(Debug, new)]
pub struct ExecuteActionsArgs<'a> {
    pub dry_run: bool,
    pub output_path: &'a Path,
    pub generator_dir: &'a Path,
    pub workspace_dir: &'a Path,
    pub current_dir: &'a Path,
    pub actions: &'a [ActionConfiguration],
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub targets: &'a UnorderedMap<String, OmniPath>,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub overwrite: Option<OverwriteConfiguration>,
    pub available_generators: &'a [GeneratorConfiguration],
}

pub async fn execute_actions<'a>(
    args: &ExecuteActionsArgs<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let mut tera_context = get_tera_context(args.context_values);

    let output_path = args.current_dir.join(args.output_path);
    let expanded_output = omni_tera::one_off(
        &output_path.to_string_lossy(),
        "output_path",
        &tera_context,
    )?;
    let output_path = Path::new(&expanded_output);

    let output_path_rel = if output_path.starts_with(args.workspace_dir) {
        output_path
            .strip_prefix(args.workspace_dir)
            .expect("should be within workspace dir")
    } else {
        &output_path
    };
    tera_context.insert("output_path", &output_path_rel);

    for (index, action) in args.actions.iter().enumerate() {
        let action_name = get_action_name(index, action, &tera_context)?;

        if skip(&action_name, action, &tera_context)? {
            trace::info!("Action {}: Skipped", &action_name);
            continue;
        }

        let handler_context = HandlerContext {
            context_values: args.context_values,
            tera_context_values: &tera_context,
            dry_run: args.dry_run,
            output_path: output_path,
            generator_targets: args.targets,
            target_overrides: args.target_overrides,
            generator_dir: args.generator_dir,
            overwrite: args.overwrite,
            available_generators: args.available_generators,
            workspace_dir: args.workspace_dir,
            resolved_action_name: action_name.as_str(),
            current_dir: args.current_dir,
        };

        let in_progress_message =
            get_in_progress_message(&action_name, action, &tera_context)?;

        trace::info!("Action {}: {}", &action_name, in_progress_message);

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
        };

        if let Err(e) = result {
            let error_message =
                get_error_message(&action_name, &e, action, &tera_context)?;

            trace::error!("Action {}: {}", &action_name, error_message);

            return Err(e);
        } else {
            let success_message =
                get_success_message(&action_name, action, &tera_context)?;

            trace::info!("Action {}: {}", &action_name, success_message);
        }
    }

    Ok(())
}

fn skip(
    name: &str,
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<bool, Error> {
    let if_expr = get_if_expr(action);

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

fn get_if_expr(action: &ActionConfiguration) -> Option<&str> {
    match action {
        ActionConfiguration::Add { action } => action.base.base.r#if.as_deref(),
        ActionConfiguration::AddContent { action } => {
            action.base.base.r#if.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.r#if.as_deref()
        }
        ActionConfiguration::RunGenerator { action } => {
            action.base.r#if.as_deref()
        }
        ActionConfiguration::Modify { action } => action.base.r#if.as_deref(),
        ActionConfiguration::Append { action } => action.base.r#if.as_deref(),
        ActionConfiguration::ModifyContent { action } => {
            action.base.r#if.as_deref()
        }
        ActionConfiguration::AppendContent { action } => {
            action.base.r#if.as_deref()
        }
        ActionConfiguration::Prepend { action } => action.base.r#if.as_deref(),
        ActionConfiguration::PrependContent { action } => {
            action.base.r#if.as_deref()
        }
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
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let result = omni_tera::one_off(message, name, tera_context)?;

    Ok(result)
}

fn get_error_message(
    action_name: &str,
    error: &Error,
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.error_message.as_deref()
        }
        ActionConfiguration::AddContent { action } => {
            action.base.base.error_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.error_message.as_deref()
        }
        ActionConfiguration::RunGenerator { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::Modify { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::Append { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::ModifyContent { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::AppendContent { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::Prepend { action } => {
            action.base.error_message.as_deref()
        }
        ActionConfiguration::PrependContent { action } => {
            action.base.error_message.as_deref()
        }
    };

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
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.in_progress_message.as_deref()
        }
        ActionConfiguration::AddContent { action } => {
            action.base.base.in_progress_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.in_progress_message.as_deref()
        }
        ActionConfiguration::RunGenerator { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::Modify { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::Append { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::ModifyContent { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::AppendContent { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::Prepend { action } => {
            action.base.in_progress_message.as_deref()
        }
        ActionConfiguration::PrependContent { action } => {
            action.base.in_progress_message.as_deref()
        }
    };

    if let Some(message) = message {
        render_text(
            message,
            &&format!("in_progress_message for action {}", action_name),
            tera_context,
        )
    } else {
        Ok("Executing action...".to_string())
    }
}

fn get_success_message(
    action_name: &str,
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.success_message.as_deref()
        }
        ActionConfiguration::AddContent { action } => {
            action.base.base.success_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.success_message.as_deref()
        }
        ActionConfiguration::RunGenerator { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::Modify { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::Append { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::ModifyContent { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::AppendContent { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::Prepend { action } => {
            action.base.success_message.as_deref()
        }
        ActionConfiguration::PrependContent { action } => {
            action.base.success_message.as_deref()
        }
    };

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
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let name = match action {
        ActionConfiguration::Add { action } => action.base.base.name.as_deref(),
        ActionConfiguration::AddContent { action } => {
            action.base.base.name.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.name.as_deref()
        }
        ActionConfiguration::RunGenerator { action } => {
            action.base.name.as_deref()
        }
        ActionConfiguration::Modify { action } => action.base.name.as_deref(),
        ActionConfiguration::Append { action } => action.base.name.as_deref(),
        ActionConfiguration::ModifyContent { action } => {
            action.base.name.as_deref()
        }
        ActionConfiguration::AppendContent { action } => {
            action.base.name.as_deref()
        }
        ActionConfiguration::Prepend { action } => action.base.name.as_deref(),
        ActionConfiguration::PrependContent { action } => {
            action.base.name.as_deref()
        }
    };

    if let Some(name) = name {
        render_text(name, &format!("name for action#{}", index), tera_context)
    } else {
        let action_type = action.discriminant();
        Ok(format!("#{}-{}", index + 1, action_type))
    }
}
