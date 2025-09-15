use std::sync::Arc;

use futures::future::try_join_all;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::{io::AsyncReadExt as _, sync::Mutex};

use crate::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterReader, MuxOutputPresenterWriter,
    StreamHandle, stream, stream_driver_handle::StreamDriverError,
    utils::TasksMap,
};

#[derive(Debug)]
pub struct StreamPresenter {
    tasks: Arc<Mutex<TasksMap<StreamPresenterError>>>,
}

impl StreamPresenter {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(TasksMap::default())),
        }
    }
}

impl StreamPresenter {
    async fn clear_closed_tasks(&self) {
        self.tasks.lock().await.retain(|_, j| j.is_finished());
    }
}

#[async_trait::async_trait]
impl MuxOutputPresenter for StreamPresenter {
    type Error = StreamPresenterError;

    async fn add_stream(
        &self,
        id: String,
        reader: Box<dyn MuxOutputPresenterReader>,
        _writer: Option<Box<dyn MuxOutputPresenterWriter>>,
    ) -> Result<StreamHandle, Self::Error> {
        self.clear_closed_tasks().await;

        let (handle, driver) = stream::handle();

        let join_handle = {
            let id = id.clone();
            let tasks = self.tasks.clone();
            tokio::spawn(async move {
                let mut stdout = tokio::io::stdout();

                tokio::io::copy(&mut reader.take(u64::MAX), &mut stdout)
                    .await?;

                driver.mark_completed().await?;

                tasks.lock().await.remove(&id);

                Ok::<(), Self::Error>(())
            })
        };

        self.tasks.lock().await.insert(id, join_handle);

        return Ok(handle);
    }

    #[inline(always)]
    fn accepts_input(&self) -> bool {
        false
    }

    async fn wait(&self) -> Result<(), Self::Error> {
        let all_values = {
            let mut tasks = self.tasks.lock().await;
            if tasks.is_empty() {
                return Ok(());
            }

            tasks.drain().map(|(_, j)| j).collect::<Vec<_>>()
        };

        let values = try_join_all(all_values).await?;

        for value in values {
            value?;
        }

        Ok(())
    }

    async fn close(self) -> Result<(), Self::Error> {
        self.wait().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stream presenter error: {inner}")]
pub struct StreamPresenterError {
    kind: StreamPresenterErrorKind,
    #[source]
    inner: StreamPresenterErrorInner,
}

impl<T: Into<StreamPresenterErrorInner>> From<T> for StreamPresenterError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(StreamPresenterErrorKind))]
enum StreamPresenterErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    StreamDriver(#[from] StreamDriverError),
}
