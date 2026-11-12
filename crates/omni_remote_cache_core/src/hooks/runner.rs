use std::sync::Arc;

use axum::response::{IntoResponse as _, Response};
use futures::future::{Either, select};

use super::{
    ArtifactHooks, Background, FailurePolicy, HookContext, HookDecision,
    HookError, HookOutcome,
};
use crate::{providers::DependencyProvider, routes::v1::artifacts::common};

async fn with_timeout<T>(
    background: Option<&Arc<dyn Background>>,
    timeout: std::time::Duration,
    fut: impl Future<Output = Result<T, HookError>>,
) -> Result<T, HookError> {
    let Some(bg) = background else {
        return fut.await;
    };
    let fut = Box::pin(fut);
    match select(fut, bg.sleep(timeout)).await {
        Either::Left((r, _)) => r,
        Either::Right(_) => {
            Err(HookError(eyre::eyre!("hook timed out after {timeout:?}")))
        }
    }
}

/// Runs the `before` hook. Returns a response when the request must stop.
pub async fn run_before(
    provider: &dyn DependencyProvider,
    ctx: &HookContext,
) -> Option<Response> {
    let settings = provider.hook_settings();
    let hooks = provider.hooks();
    let bg = provider.background();

    match with_timeout(bg.as_ref(), settings.before_timeout, hooks.before(ctx))
        .await
    {
        Ok(HookDecision::Allow) => None,
        Ok(HookDecision::Veto { reason }) => {
            tracing::warn!(op = %ctx.op, %reason, "request vetoed by hook");
            Some(common::forbidden_response(Some(&reason)))
        }
        Err(e) => match settings.before_policy {
            FailurePolicy::FailOpen => {
                tracing::error!(op = %ctx.op, error = %e, "before hook failed, failing open");
                None
            }
            FailurePolicy::FailClosed => {
                tracing::error!(op = %ctx.op, error = %e, "before hook failed, failing closed");
                Some(
                    axum_extra::response::InternalServerError(e)
                        .into_response(),
                )
            }
        },
    }
}

/// Runs the `after` hook off the request path when a `Background` is
/// available, otherwise inline. Failures are logged, never surfaced.
pub async fn run_after(
    provider: &dyn DependencyProvider,
    ctx: HookContext,
    outcome: HookOutcome,
) {
    let settings = provider.hook_settings();
    let hooks: Arc<dyn ArtifactHooks> = provider.hooks();
    let bg = provider.background();

    let task = {
        let bg = bg.clone();
        async move {
            let r = with_timeout(
                bg.as_ref(),
                settings.after_timeout,
                hooks.after(&ctx, &outcome),
            )
            .await;
            if let Err(e) = r {
                tracing::error!(op = %ctx.op, error = %e, "after hook failed");
            }
        }
    };

    match bg {
        Some(bg) => bg.spawn(Box::pin(task)),
        None => task.await,
    }
}
