use crate::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterReader, MuxOutputPresenterWriter,
    StreamHandle, StreamPresenter, StreamPresenterError,
};

#[derive(Debug)]
pub enum MuxOutputPresenterStatic {
    Stream(StreamPresenter),
}

impl From<StreamPresenter> for MuxOutputPresenterStatic {
    fn from(value: StreamPresenter) -> Self {
        Self::Stream(value)
    }
}

#[async_trait::async_trait]
impl MuxOutputPresenter for MuxOutputPresenterStatic {
    type Error = MuxOutputPresenterError;

    fn add_stream(
        &self,
        id: String,
        reader: Box<dyn MuxOutputPresenterReader>,
    ) -> Result<StreamHandle, Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => s.add_stream(id, reader)?,
        })
    }

    fn register_input_writer(
        &self,
        writer: Box<dyn MuxOutputPresenterWriter>,
    ) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => {
                s.register_input_writer(writer)?
            }
        })
    }

    fn accepts_input(&self) -> bool {
        match self {
            MuxOutputPresenterStatic::Stream(s) => s.accepts_input(),
        }
    }

    async fn wait(&self) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => s.wait().await?,
        })
    }

    async fn close(&self) -> Result<(), Self::Error> {
        Ok(match self {
            MuxOutputPresenterStatic::Stream(s) => s.close().await?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MuxOutputPresenterError {
    #[error(transparent)]
    Stream(#[from] StreamPresenterError),
}
