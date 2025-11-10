use std::path::{Path, PathBuf};

use derive_new::new;
use maps::UnorderedMap;
use omni_generator_configurations::ActionConfiguration;
use strum::IntoDiscriminant;
use value_bag::OwnedValueBag;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, add, add_inline, add_many},
    error::{Error, ErrorInner},
    utils::get_tera_context,
};

#[derive(Debug, new)]
pub struct ExecuteActionsArgs<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub generator_dir: &'a Path,
    pub actions: &'a [ActionConfiguration],
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub targets: &'a UnorderedMap<String, PathBuf>,
}

pub async fn execute_actions<'a>(
    args: &ExecuteActionsArgs<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let tera_context = get_tera_context(args.context_values);

    for (index, action) in args.actions.iter().enumerate() {
        let action_name = get_action_name(index, action, &tera_context)?;

        if skip(action, &tera_context)? {
            trace::info!("Action {}: Skipped", &action_name);
            continue;
        }

        let handler_context = HandlerContext {
            context_values: args.context_values,
            tera_context_values: &tera_context,
            dry_run: args.dry_run,
            output_dir: args.output_dir,
            generator_targets: args.targets,
            project_targets: args.targets,
            generator_dir: args.generator_dir,
        };

        let in_progress_message =
            get_in_progress_message(action, &tera_context)?;

        trace::info!("Action {}: {}", &action_name, in_progress_message);

        let result = match action {
            ActionConfiguration::Add { action } => {
                add(action, &handler_context, sys).await
            }
            ActionConfiguration::AddInline { action } => {
                add_inline(action, &handler_context, sys).await
            }
            ActionConfiguration::AddMany { action } => {
                add_many(action, &handler_context, sys).await
            }
        };

        if let Err(e) = result {
            let error_message = get_error_message(&e, action, &tera_context)?;

            trace::error!("Action {}: {}", &action_name, error_message);

            return Err(e);
        } else {
            let success_message = get_success_message(action, &tera_context)?;

            trace::info!("Action {}: {}", &action_name, success_message);
        }
    }

    Ok(())
}

fn skip(
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<bool, Error> {
    let if_expr = get_if_expr(action);

    if let Some(if_expr) = if_expr {
        let result = tera::Tera::one_off(if_expr, tera_context, false)?;
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
        ActionConfiguration::AddInline { action } => {
            action.base.base.r#if.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.r#if.as_deref()
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
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let result = tera::Tera::one_off(message, tera_context, false)?;

    Ok(result)
}

fn get_error_message(
    error: &Error,
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.error_message.as_deref()
        }
        ActionConfiguration::AddInline { action } => {
            action.base.base.error_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.error_message.as_deref()
        }
    };

    if let Some(message) = message {
        let mut error_ctx = tera_context.clone();
        error_ctx.insert("error", &error.to_string());

        render_text(message, &error_ctx)
    } else {
        Ok(format!(
            "Encountered an error while executing action: {}",
            error.to_string()
        ))
    }
}

fn get_in_progress_message(
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.in_progress_message.as_deref()
        }
        ActionConfiguration::AddInline { action } => {
            action.base.base.in_progress_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.in_progress_message.as_deref()
        }
    };

    if let Some(message) = message {
        render_text(message, tera_context)
    } else {
        Ok("Executing action...".to_string())
    }
}

fn get_success_message(
    action: &ActionConfiguration,
    tera_context: &tera::Context,
) -> Result<String, Error> {
    let message = match action {
        ActionConfiguration::Add { action } => {
            action.base.base.success_message.as_deref()
        }
        ActionConfiguration::AddInline { action } => {
            action.base.base.success_message.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.success_message.as_deref()
        }
    };

    if let Some(message) = message {
        render_text(message, tera_context)
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
        ActionConfiguration::AddInline { action } => {
            action.base.base.name.as_deref()
        }
        ActionConfiguration::AddMany { action } => {
            action.base.base.name.as_deref()
        }
    };

    if let Some(name) = name {
        render_text(name, tera_context)
    } else {
        let action_type = action.discriminant();
        Ok(format!("#{}-{}", index + 1, action_type))
    }
}
