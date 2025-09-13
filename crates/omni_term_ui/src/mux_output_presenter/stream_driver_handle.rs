use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::{Receiver, Sender, error::RecvError};

#[derive(Debug, new)]
pub struct StreamDriverHandle {
    #[allow(unused)]
    stop_signal: Receiver<()>,
    wait_signal: Sender<()>,
}

impl StreamDriverHandle {
    #[allow(unused)]
    pub fn is_stopped(&self) -> bool {
        self.stop_signal.is_terminated()
    }

    pub async fn mark_completed(self) -> Result<(), StreamDriverError> {
        if self.wait_signal.is_closed() {
            return Ok(());
        }

        self.wait_signal
            .send(())
            .map_err(|_| StreamDriverErrorInner::FailedToMarkCompleted)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stream driver error: {inner}")]
pub struct StreamDriverError {
    kind: StreamDriverErrorKind,
    #[source]
    inner: StreamDriverErrorInner,
}

impl<T: Into<StreamDriverErrorInner>> From<T> for StreamDriverError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

/// Error type for stream driver.
#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(StreamDriverErrorKind))]
enum StreamDriverErrorInner {
    #[error("failed to wait for stream stop")]
    WaitForStop(#[from] RecvError),

    // test - test - test - test - test
    #[error("failed to send stream stop")]
    FailedToMarkCompleted,
}
