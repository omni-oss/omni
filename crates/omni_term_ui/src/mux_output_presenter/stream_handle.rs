use std::{pin::Pin, task::Poll};

use derive_new::new;
use futures::FutureExt;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::{Receiver, Sender, error::RecvError};

#[derive(Debug, new)]
pub struct StreamHandle {
    stop_signal: Sender<()>,
    wait_signal: Receiver<()>,
}

#[inline(always)]
async fn wait(wait_signal: Receiver<()>) -> Result<(), StreamHandleError> {
    if wait_signal.is_terminated() {
        return Ok(());
    }

    wait_signal.await.map_err(|e| e.into())
}

impl StreamHandle {
    /// Stops the stream, and waits for it to stop.
    pub async fn stop(self) -> Result<(), StreamHandleError> {
        // already stopped so ignore it
        if self.stop_signal.is_closed() {
            wait(self.wait_signal).await?;
            return Ok(());
        }

        self.stop_signal
            .send(())
            .map_err(|_| StreamHandleErrorInner::FailedToStop)?;
        wait(self.wait_signal).await?;

        Ok(())
    }

    /// Wait for the stream to finish processing.
    pub async fn wait(self) -> Result<(), StreamHandleError> {
        wait(self.wait_signal).await
    }
}

impl Future for StreamHandle {
    type Output = Result<(), StreamHandleError>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        self.wait_signal.poll_unpin(cx).map_err(|e| e.into())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stream handle error: {inner}")]
pub struct StreamHandleError {
    kind: StreamHandleErrorKind,
    #[source]
    inner: StreamHandleErrorInner,
}

impl<T: Into<StreamHandleErrorInner>> From<T> for StreamHandleError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(StreamHandleErrorKind))]
enum StreamHandleErrorInner {
    #[error("failed to stop stream")]
    FailedToStop,

    #[error("failed to wait for stream")]
    FailedToWait(#[from] RecvError),
}
