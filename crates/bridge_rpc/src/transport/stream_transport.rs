use std::collections::VecDeque;

use bytes::Bytes;
use strum::{EnumDiscriminants, IntoDiscriminant};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};

use crate::{
    Id, Transport, TransportReadFramer, TransportWriteFramer,
    constants::STREAM_BUFFER_SIZE,
};

pub struct StreamTransport<TInput, TOutput>
where
    TInput: AsyncRead,
    TOutput: AsyncWrite,
{
    #[allow(unused)]
    id: Id,
    input: Mutex<TInput>,
    output: Mutex<TOutput>,
    write_framer: TransportWriteFramer,
    read_framer: Mutex<TransportReadFramer>,
    buffered_frames: Mutex<VecDeque<Bytes>>,
}

impl<TInput, TOutput> StreamTransport<TInput, TOutput>
where
    TInput: AsyncRead,
    TOutput: AsyncWrite,
{
    pub fn new(input: TInput, output: TOutput) -> Self {
        Self {
            id: Id::new(),
            input: Mutex::new(input),
            output: Mutex::new(output),
            write_framer: TransportWriteFramer::new(),
            read_framer: Mutex::new(TransportReadFramer::new()),
            buffered_frames: Mutex::new(VecDeque::new()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct StreamTransportError(pub(crate) StreamTransportErrorInner);

impl StreamTransportError {
    pub fn kind(&self) -> StreamTransportErrorKind {
        self.0.discriminant()
    }
}

impl<T> From<T> for StreamTransportError
where
    T: Into<StreamTransportErrorInner>,
{
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(StreamTransportErrorKind), vis(pub))]
pub(crate) enum StreamTransportErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("end of stream")]
    EndOfStream,

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}

#[async_trait::async_trait]
impl<TInput, TOutput> Transport for StreamTransport<TInput, TOutput>
where
    TInput: AsyncRead + Send + Sync + Unpin + 'static,
    TOutput: AsyncWrite + Send + Sync + Unpin + 'static,
{
    type Error = StreamTransportError;

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(transport_id = ?self.id)))]
    async fn send(&self, data: Bytes) -> Result<(), Self::Error> {
        let frame = self.write_framer.frame(data);
        let mut output = self.output.lock().await;
        output.write_all(&frame.length).await?;
        output.write_all(&frame.data).await?;
        trace::trace!(
            bytes_sent = frame.length.len() + frame.data.len(),
            "sent frame"
        );
        Ok(())
    }

    #[cfg_attr(feature = "enable-tracing", tracing::instrument(skip_all, fields(transport_id = ?self.id)))]
    async fn receive(&self) -> Result<Bytes, Self::Error> {
        trace::trace!("starting receive");
        // If we have a buffered frame, return it and remove it from the buffer
        if let Some(frame) = self.buffered_frames.lock().await.pop_front() {
            trace::trace!("got frame from buffer, returning");
            return Ok(frame);
        }

        let mut buf = [0; STREAM_BUFFER_SIZE];
        loop {
            trace::trace!("reading from input");
            let n_bytes_read = self.input.lock().await.read(&mut buf).await?;
            trace::trace!(bytes_read = n_bytes_read, "received bytes");

            if n_bytes_read == 0 {
                let mut read_framer = self.read_framer.lock().await;
                if let Some(frame) = read_framer.frame(Bytes::new()) {
                    self.buffered_frames.lock().await.extend(frame);
                } else {
                    read_framer.reset();
                }

                if let Some(frame) =
                    self.buffered_frames.lock().await.pop_front()
                {
                    trace::trace!("got frame from buffer, returning");
                    return Ok(frame);
                } else {
                    trace::error!("no frame found, returning end of stream");
                    return Err(StreamTransportErrorInner::EndOfStream.into());
                }
            }

            let frame = self
                .read_framer
                .lock()
                .await
                .frame(Bytes::copy_from_slice(&buf[..n_bytes_read]));

            if let Some(frame) = frame
                && !frame.is_empty()
            {
                trace::trace!("completed frame from framer, add to buffer");
                self.buffered_frames.lock().await.extend(frame);
            }

            if let Some(frame) = self.buffered_frames.lock().await.pop_front() {
                trace::trace!("got frame from buffer, returning");
                return Ok(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::io::{AsyncReadExt, AsyncWriteExt as _};

    use crate::{
        StreamTransport, Transport as _,
        constants::{LENGTH_PREFIX_LENGTH, STREAM_BUFFER_SIZE},
    };

    #[tokio::test]
    async fn test_send_single_frame() {
        // Simulate sending a framed message from the server side
        let (_input_in, input_out) = tokio::io::duplex(STREAM_BUFFER_SIZE * 2);
        let (output_in, mut output_out) =
            tokio::io::duplex(STREAM_BUFFER_SIZE * 2);
        let transport = StreamTransport::new(input_out, output_in);

        let data = b"hello world";

        transport
            .send(Bytes::from_static(data))
            .await
            .expect("send failed");

        let mut buf = [0u8; STREAM_BUFFER_SIZE];
        let n = output_out.read(&mut buf[..]).await.expect("read failed");

        let len = u32::from_le_bytes(
            buf[..LENGTH_PREFIX_LENGTH]
                .try_into()
                .expect("slice length mismatch"),
        );

        assert_eq!(len, data.len() as u32);
        assert_eq!(&buf[LENGTH_PREFIX_LENGTH..n], data);
    }

    #[tokio::test]
    async fn test_receive_single_frame() {
        // Simulate sending a framed message from the server side
        let (input_in, mut input_out) =
            tokio::io::duplex(STREAM_BUFFER_SIZE * 2);
        let (_output_in, output_out) =
            tokio::io::duplex(STREAM_BUFFER_SIZE * 2);
        let transport = StreamTransport::new(input_in, output_out);

        // Send a framed message
        let data = b"test frame data";
        let framed_len = (data.len() as u32).to_le_bytes();
        input_out
            .write_all(&framed_len)
            .await
            .expect("write failed");
        input_out.write_all(data).await.expect("write failed");

        let received_data = transport.receive().await.expect("receive failed");
        assert_eq!(received_data, Bytes::from_static(data));
    }
}
