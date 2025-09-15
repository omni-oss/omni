use std::error::Error;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::mux_output_presenter::StreamHandle;

#[system_traits::auto_impl]
pub trait MuxOutputPresenterReader:
    AsyncRead + Unpin + Send + Sync + 'static
{
}

#[system_traits::auto_impl]
pub trait MuxOutputPresenterWriter:
    AsyncWrite + Unpin + Send + Sync + 'static
{
}

#[async_trait::async_trait]
pub trait MuxOutputPresenter: Send + Sync {
    type Error: Error;

    /// Add a new output stream to be multiplexed, identified by a string id.
    async fn add_stream(
        &self,
        id: String,
        reader: Box<dyn MuxOutputPresenterReader>,
        writer: Option<Box<dyn MuxOutputPresenterWriter>>,
    ) -> Result<StreamHandle, Self::Error>;

    /// Whether this presenter consumes user input/events (e.g. keyboard, UI events).
    fn accepts_input(&self) -> bool;

    async fn wait(&self) -> Result<(), Self::Error>;

    async fn close(self) -> Result<(), Self::Error>;
}

#[async_trait::async_trait]
pub trait MuxOutputPresenterExt: MuxOutputPresenter {
    #[inline(always)]
    async fn add_stream_output<I, R>(
        &self,
        id: I,
        reader: R,
    ) -> Result<StreamHandle, Self::Error>
    where
        R: MuxOutputPresenterReader,
        I: Into<String> + Send + Sync,
    {
        self.add_stream(id.into(), Box::new(reader), None).await
    }

    #[inline(always)]
    async fn add_stream_full<I, R, W>(
        &self,
        id: I,
        output: R,
        input: W,
    ) -> Result<StreamHandle, Self::Error>
    where
        I: Into<String> + Send + Sync,
        R: MuxOutputPresenterReader,
        W: MuxOutputPresenterWriter,
    {
        self.add_stream(id.into(), Box::new(output), Some(Box::new(input)))
            .await
    }

    #[inline(always)]
    async fn add_piped_stream_output<I>(
        &self,
        id: I,
    ) -> Result<(impl AsyncWrite + 'static, StreamHandle), Self::Error>
    where
        I: Into<String> + Send + Sync,
    {
        let (reader, writer) = tokio::io::duplex(1024);
        let handle = self.add_stream_output(id, reader).await?;

        Ok((writer, handle))
    }

    #[inline(always)]
    async fn add_piped_stream_full<I>(
        &self,
        id: I,
    ) -> Result<
        (
            impl AsyncWrite + 'static,
            impl AsyncRead + 'static,
            StreamHandle,
        ),
        Self::Error,
    >
    where
        I: Into<String> + Send + Sync,
    {
        let (out_reader, out_writer) = tokio::io::duplex(1024);
        let (in_reader, in_writer) = tokio::io::duplex(1024);
        let handle = self.add_stream_full(id, out_reader, in_writer).await?;

        Ok((out_writer, in_reader, handle))
    }
}

impl<T: MuxOutputPresenter> MuxOutputPresenterExt for T {}
