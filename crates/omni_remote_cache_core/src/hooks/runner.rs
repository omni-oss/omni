use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::response::{IntoResponse as _, Response};
use futures::future::{Either, select};

use super::{
    ArtifactHooks, FailurePolicy, HookContext, HookDecision, HookError,
    HookOutcome, HostRuntime,
};
use crate::{providers::DependencyProvider, routes::v1::artifacts::common};

async fn with_timeout<T>(
    runtime: Option<&Arc<dyn HostRuntime>>,
    timeout: std::time::Duration,
    fut: impl Future<Output = Result<T, HookError>>,
) -> Result<T, HookError> {
    let Some(rt) = runtime else {
        return fut.await;
    };
    let fut = Box::pin(fut);
    match select(fut, rt.sleep(timeout)).await {
        Either::Left((r, _)) => r,
        Either::Right(_) => {
            Err(HookError(eyre::eyre!("hook timed out after {timeout:?}")))
        }
    }
}

static AFTER_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

struct InFlight;

impl InFlight {
    fn acquire(max: usize) -> Option<Self> {
        let prev = AFTER_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        if prev >= max {
            AFTER_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        AFTER_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs the `before` hook. Returns a response when the request must stop.
pub async fn run_before(
    provider: &dyn DependencyProvider,
    ctx: &HookContext,
) -> Option<Response> {
    let settings = provider.hook_settings();
    let hooks = provider.hooks();
    let rt = provider.runtime();

    match with_timeout(rt.as_ref(), settings.before_timeout, hooks.before(ctx))
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

/// Runs the `after` hook off the request path when a `HostRuntime` is
/// available, otherwise inline. Failures are logged, never surfaced.
pub async fn run_after(
    provider: &dyn DependencyProvider,
    ctx: HookContext,
    outcome: HookOutcome,
) {
    let settings = provider.hook_settings();
    let hooks: Arc<dyn ArtifactHooks> = provider.hooks();
    let rt = provider.runtime();

    let Some(permit) = InFlight::acquire(settings.after_max_in_flight) else {
        tracing::warn!(op = %ctx.op, "too many after hooks in flight, skipping");
        return;
    };

    let task = {
        let rt = rt.clone();
        async move {
            let _permit = permit;
            let r = with_timeout(
                rt.as_ref(),
                settings.after_timeout,
                hooks.after(&ctx, &outcome),
            )
            .await;
            if let Err(e) = r {
                tracing::error!(op = %ctx.op, error = %e, "after hook failed");
            }
        }
    };

    match rt {
        Some(rt) => rt.spawn_detached(Box::pin(task)),
        None => task.await,
    }
}
