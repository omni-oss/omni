use std::error::Error;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::mux_output_presenter::StreamHandle;

pub trait MuxOutputPresenterReader:
    AsyncRead + Unpin + Send + Sync + 'static
{
}
impl<T> MuxOutputPresenterReader for T where
    T: AsyncRead + Unpin + Send + Sync + 'static
{
}

pub trait MuxOutputPresenterWriter:
    AsyncWrite + Unpin + Send + Sync + 'static
{
}
impl<T> MuxOutputPresenterWriter for T where
    T: AsyncWrite + Unpin + Send + Sync + 'static
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
    ) -> Result<StreamHandle, Self::Error>;

    /// Register a writable handle for sending input to the process with `id`.
    async fn register_input_writer(
        &self,
        writer: Box<dyn MuxOutputPresenterWriter>,
    ) -> Result<(), Self::Error>;

    /// Whether this presenter consumes user input/events (e.g. keyboard, UI events).
    fn accepts_input(&self) -> bool;

    async fn wait(&self) -> Result<(), Self::Error>;

    async fn close(self) -> Result<(), Self::Error>;
}

#[async_trait::async_trait]
pub trait MuxOutputPresenterExt: MuxOutputPresenter {
    #[inline(always)]
    async fn add_stream_generic<I, R>(
        &self,
        id: I,
        reader: R,
    ) -> Result<StreamHandle, Self::Error>
    where
        R: MuxOutputPresenterReader,
        I: Into<String> + Send + Sync,
    {
        self.add_stream(id.into(), Box::new(reader)).await
    }

    #[inline(always)]
    async fn add_piped_stream<I>(
        &self,
        id: I,
    ) -> Result<(impl AsyncWrite + 'static, StreamHandle), Self::Error>
    where
        I: Into<String> + Send + Sync,
    {
        let (reader, writer) = tokio::io::duplex(1024);
        let handle = self.add_stream_generic(id, reader).await?;

        Ok((writer, handle))
    }

    #[inline(always)]
    async fn register_input_reader_generic<W>(
        &self,
        writer: W,
    ) -> Result<(), Self::Error>
    where
        W: MuxOutputPresenterWriter,
    {
        self.register_input_writer(Box::new(writer)).await
    }
}

impl<T: MuxOutputPresenter> MuxOutputPresenterExt for T {}
