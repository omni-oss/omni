use bridge_rpc_core::{DynMap, client::response::ResponseReader};

use crate::error::ReadError;

pub async fn read_response(
    mut reader: ResponseReader,
) -> Result<(Vec<u8>, Option<DynMap>), ReadError> {
    let mut chunks = Vec::new();
    while let Some(chunk) = reader.read_body_chunk().await? {
        chunks.push(chunk);
    }

    let trailers = reader.into_trailers()?;

    Ok((chunks.concat(), trailers))
}

pub async fn read_response_as_string(
    reader: ResponseReader,
) -> Result<(String, Option<DynMap>), ReadError> {
    let (bytes, trailers) = read_response(reader).await?;
    String::from_utf8(bytes)
        .map(|value| (value, trailers))
        .map_err(ReadError::custom)
}

pub async fn read_response_as_json<T: serde::de::DeserializeOwned>(
    reader: ResponseReader,
) -> Result<(T, Option<DynMap>), ReadError> {
    let (bytes, trailers) = read_response(reader).await?;
    serde_json::from_slice(&bytes)
        .map(|value| (value, trailers))
        .map_err(ReadError::custom)
}

#[cfg(test)]
mod tests {
    use bridge_rpc_core::{
        DynMap, Id, ResponseErrorCode,
        client::response::{ResponseFrameEvent, ResponseReader},
        frame::ResponseError as ResponseErrorFrame,
    };
    use serde::{Deserialize, Serialize};
    use tokio::sync::{mpsc, oneshot};

    use crate::error::ReadErrorKind;

    use super::*;

    /// Builds a [`ResponseReader`] backed by mpsc/oneshot channels and
    /// returns the senders so a test can drive frames into the reader.
    fn make_reader() -> (
        ResponseReader,
        mpsc::Sender<ResponseFrameEvent>,
        oneshot::Sender<ResponseErrorFrame>,
    ) {
        let (frame_tx, frame_rx) = mpsc::channel(64);
        let (error_tx, error_rx) = oneshot::channel();

        let reader = ResponseReader::new(Id::new(), frame_rx, error_rx);

        (reader, frame_tx, error_tx)
    }

    /// Pushes a series of body chunks followed by an `End` frame into the
    /// given sender. Panics if any send fails.
    async fn feed_body(
        sender: &mpsc::Sender<ResponseFrameEvent>,
        chunks: impl IntoIterator<Item = Vec<u8>>,
        trailers: Option<DynMap>,
    ) {
        for chunk in chunks {
            sender
                .send(ResponseFrameEvent::BodyChunk { chunk })
                .await
                .expect("failed to push body chunk");
        }

        sender
            .send(ResponseFrameEvent::End { trailers })
            .await
            .expect("failed to push end frame");
    }

    #[tokio::test]
    async fn read_response_concatenates_multiple_chunks() {
        let (reader, sender, _err) = make_reader();
        feed_body(
            &sender,
            [b"part-".to_vec(), b"one-".to_vec(), b"two".to_vec()],
            None,
        )
        .await;

        let (bytes, trailers) = read_response(reader)
            .await
            .expect("reading response should succeed");

        assert_eq!(bytes, b"part-one-two");
        assert!(trailers.is_none());
    }

    #[tokio::test]
    async fn read_response_with_empty_body_and_no_trailers() {
        let (reader, sender, _err) = make_reader();
        feed_body(&sender, [], None).await;

        let (bytes, trailers) = read_response(reader)
            .await
            .expect("reading response should succeed");

        assert!(bytes.is_empty());
        assert!(trailers.is_none());
    }

    #[tokio::test]
    async fn read_response_returns_trailers() {
        let (reader, sender, _err) = make_reader();
        let mut trailers = DynMap::new();
        trailers.insert_raw("x-server", "omni");

        feed_body(&sender, [b"payload".to_vec()], Some(trailers)).await;

        let (bytes, trailers) = read_response(reader)
            .await
            .expect("reading response should succeed");

        assert_eq!(bytes, b"payload");
        let trailers = trailers.expect("trailers should be present");
        assert!(
            trailers.has_key("x-server"),
            "x-server trailer should be present",
        );
    }

    #[tokio::test]
    async fn read_response_propagates_response_error_frame() {
        let (reader, sender, error_tx) = make_reader();

        // Push an error before any frames so the reader observes it on the
        // next read.
        error_tx
            .send(ResponseErrorFrame::new(
                Id::new(),
                ResponseErrorCode::UNEXPECTED_FRAME,
                "server failure".to_string(),
            ))
            .expect("failed to send error");

        // Drop the sender so that the reader's `recv` loop won't block in
        // case the error is observed only after a frame attempt.
        drop(sender);

        let err = read_response(reader)
            .await
            .expect_err("expected an error to propagate");

        assert!(matches!(err.kind(), ReadErrorKind::Response));
    }

    #[tokio::test]
    async fn read_response_as_string_decodes_valid_utf8() {
        let (reader, sender, _err) = make_reader();
        feed_body(
            &sender,
            ["hi, ".as_bytes().to_vec(), "世界".as_bytes().to_vec()],
            None,
        )
        .await;

        let (text, trailers) = read_response_as_string(reader)
            .await
            .expect("reading should succeed");

        assert_eq!(text, "hi, 世界");
        assert!(trailers.is_none());
    }

    #[tokio::test]
    async fn read_response_as_string_errors_on_invalid_utf8() {
        let (reader, sender, _err) = make_reader();
        // 0xff is never a valid leading UTF-8 byte.
        feed_body(&sender, [vec![0xffu8, 0xfe, 0xfd]], None).await;

        let err = read_response_as_string(reader)
            .await
            .expect_err("expected utf-8 decode error");

        assert!(matches!(err.kind(), ReadErrorKind::Custom));
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Payload {
        ok: bool,
        message: String,
    }

    #[tokio::test]
    async fn read_response_as_json_decodes_valid_payload() {
        let payload = Payload {
            ok: true,
            message: "all good".to_string(),
        };
        let bytes = serde_json::to_vec(&payload).unwrap();

        let (reader, sender, _err) = make_reader();
        // Split the JSON across two chunks to verify that concatenation
        // happens before deserialization.
        let (left, right) = bytes.split_at(bytes.len() / 2);
        feed_body(&sender, [left.to_vec(), right.to_vec()], None).await;

        let (decoded, trailers) = read_response_as_json::<Payload>(reader)
            .await
            .expect("reading json should succeed");

        assert_eq!(decoded, payload);
        assert!(trailers.is_none());
    }

    #[tokio::test]
    async fn read_response_as_json_errors_on_invalid_json() {
        let (reader, sender, _err) = make_reader();
        feed_body(&sender, [b"this is not json".to_vec()], None).await;

        let err = read_response_as_json::<Payload>(reader)
            .await
            .expect_err("expected json decode error");

        assert!(matches!(err.kind(), ReadErrorKind::Custom));
    }

    #[tokio::test]
    async fn read_response_as_json_preserves_trailers() {
        let payload = Payload {
            ok: false,
            message: "with trailers".to_string(),
        };
        let bytes = serde_json::to_vec(&payload).unwrap();

        let (reader, sender, _err) = make_reader();
        let mut trailers = DynMap::new();
        trailers.insert_raw("x-status", "done");

        feed_body(&sender, [bytes], Some(trailers)).await;

        let (decoded, trailers) = read_response_as_json::<Payload>(reader)
            .await
            .expect("reading json should succeed");

        assert_eq!(decoded, payload);
        let trailers = trailers.expect("trailers should be present");
        assert!(
            trailers.has_key("x-status"),
            "x-status trailer should be present",
        );
    }
}
