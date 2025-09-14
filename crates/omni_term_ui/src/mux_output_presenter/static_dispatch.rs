use strum::EnumIs;

use crate::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterReader, MuxOutputPresenterWriter,
    StreamHandle, StreamPresenter, StreamPresenterError, TuiPresenter,
    TuiPresenterError,
};

#[derive(EnumIs)]
pub enum MuxOutputPresenterStatic {
    Stream(StreamPresenter),
    Tui(TuiPresenter),
}

impl MuxOutputPresenterStatic {
    pub fn new_stream() -> Self {
        Self::Stream(StreamPresenter::new())
    }

    pub fn new_tui() -> Self {
        Self::Tui(TuiPresenter::new())
    }
}

impl From<StreamPresenter> for MuxOutputPresenterStatic {
    fn from(value: StreamPresenter) -> Self {
        Self::Stream(value)
    }
}

impl From<TuiPresenter> for MuxOutputPresenterStatic {
    fn from(value: TuiPresenter) -> Self {
        Self::Tui(value)
    }
}

#[async_trait::async_trait]
impl MuxOutputPresenter for MuxOutputPresenterStatic {
    type Error = MuxOutputPresenterError;

    async fn add_stream(
        &self,
        id: String,
        reader: Box<dyn MuxOutputPresenterReader>,
    ) -> Result<StreamHandle, Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => {
                s.add_stream(id, reader).await?
            }
            MuxOutputPresenterStatic::Tui(t) => {
                t.add_stream(id, reader).await?
            }
        })
    }

    async fn register_input_writer(
        &self,
        writer: Box<dyn MuxOutputPresenterWriter>,
    ) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => {
                s.register_input_writer(writer).await?
            }
            MuxOutputPresenterStatic::Tui(t) => {
                t.register_input_writer(writer).await?
            }
        })
    }

    fn accepts_input(&self) -> bool {
        match self {
            MuxOutputPresenterStatic::Stream(s) => s.accepts_input(),
            MuxOutputPresenterStatic::Tui(t) => t.accepts_input(),
        }
    }

    async fn wait(&self) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => s.wait().await?,
            MuxOutputPresenterStatic::Tui(t) => t.wait().await?,
        })
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => s.close().await?,
            MuxOutputPresenterStatic::Tui(t) => t.close().await?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MuxOutputPresenterError {
    #[error(transparent)]
    Stream(#[from] StreamPresenterError),
    #[error(transparent)]
    Tui(#[from] TuiPresenterError),
}
