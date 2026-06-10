use std::time::Duration;

use async_trait::async_trait;
use bridge_rpc_core::{
    BridgeRpc, DynMap, ResponseStatusCode, StreamTransport,
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use derive_new::new;
use ntest::timeout;
use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    time::sleep,
};

use bridge_rpc_core::{
    Id, ResponseErrorCode, TransportWriteFramer, bridge::frame::Frame,
};

const TEST_PATH: &str = "test_path";

/// Creates an RPC whose transport is connected to two raw duplex streams
/// that the test controls directly (no length-prefix framing is added by
/// the helper – the helpers below do that).
fn create_rpc_with_raw_transport(
    service: MirrorTestService,
) -> (
    BridgeRpc<StreamTransport<DuplexStream, DuplexStream>, MirrorTestService>,
    DuplexStream, // our_writer  – write here to send frames TO the RPC
    DuplexStream, // our_reader  – read here to see what the RPC sends back
) {
    let (rpc_reader, our_writer) = tokio::io::duplex(8192);
    let (our_reader, rpc_writer) = tokio::io::duplex(8192);
    let transport = StreamTransport::new(rpc_reader, rpc_writer);
    let rpc = BridgeRpc::new(transport, service);
    (rpc, our_writer, our_reader)
}

/// Serialise `frame` as msgpack and write it with the 4-byte LE length
/// prefix that `StreamTransport` expects.
async fn write_frame(writer: &mut DuplexStream, frame: &Frame) {
    let bytes = rmp_serde::to_vec(frame).expect("Failed to serialize frame");
    let framer = TransportWriteFramer::new();
    let framed = framer.frame(bytes::Bytes::from(bytes));
    writer
        .write_all(&framed.length)
        .await
        .expect("Failed to write length");
    writer
        .write_all(&framed.data)
        .await
        .expect("Failed to write data");
}

/// Read one length-prefixed frame from `reader`.
async fn read_frame(reader: &mut DuplexStream) -> Frame {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .expect("Failed to read length");
    let len = u32::from_le_bytes(len_buf);
    let mut data_buf = vec![0u8; len as usize];
    reader
        .read_exact(&mut data_buf)
        .await
        .expect("Failed to read data");
    rmp_serde::from_slice(&data_buf).expect("Failed to deserialize frame")
}

/// A service that silently drains the request body (handling aborted
/// sessions without panicking) and never starts a response.  Used when
/// the test needs the service task to exit cleanly even if the session
/// is forcibly closed before a proper End frame arrives.
#[derive(new)]
struct NoopTestService {}

#[async_trait]
impl Service for NoopTestService {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let mut reader = context.request.into_reader();
        // Drain until EOF or channel-closed; never call trailers().
        while let Ok(Some(_)) = reader.read_body_chunk().await {}
        // PendingResponse is simply dropped; no response frames are emitted.
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_ping() {
    let (rpc1, rpc2) = create_mirror_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    let result1 = rpc1
        .ping(Duration::from_millis(100))
        .await
        .expect("rpc1 ping failed");

    let result2 = rpc2
        .ping(Duration::from_millis(100))
        .await
        .expect("rpc2 ping failed");

    assert!(result1, "ping should return true");
    assert!(result2, "ping should return true");

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_request() {
    let (rpc1, rpc2) = create_mirror_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    let request_data = create_data();
    let serialized_data =
        rmp_serde::to_vec(&request_data).expect("Failed to serialize data");
    let headers = create_headers();
    let trailers = create_trailers();
    let path = "test_path";

    let mut active_request = rpc1
        .request(path)
        .await
        .expect("Request failed")
        .start_with_headers(headers.clone())
        .await
        .expect("Failed to start response");

    active_request
        .write_body_chunk(serialized_data.clone())
        .await
        .expect("Failed to write body chunk");

    let response = active_request
        .end_with_trailers(trailers.clone())
        .await
        .expect("Failed to end request")
        .wait()
        .await
        .expect("failed to wait for response");

    let (status, response_headers, mut reader) = response.into_parts();

    let mut response_data_serialized = vec![];
    while let Some(chunk) = reader
        .read_body_chunk()
        .await
        .expect("Failed to read chunk")
    {
        response_data_serialized.extend_from_slice(&chunk);
    }

    let response_data: RpcData =
        rmp_serde::from_slice(&response_data_serialized)
            .expect("Failed to deserialize data");

    let response_trailers = reader
        .trailers()
        .expect("Failed to get trailers")
        .map(|t| t.clone());

    assert_eq!(status, ResponseStatusCode::SUCCESS);
    assert_eq!(Some(headers), response_headers);
    assert_eq!(Some(trailers), response_trailers);
    assert_eq!(response_data, request_data);

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_response() {
    #[derive(new)]
    struct ResponseTestService {}

    #[async_trait]
    impl Service for ResponseTestService {
        async fn run(
            &self,
            context: ServiceContext,
        ) -> Result<(), ServiceError> {
            consume_request(context.request).await;

            write_response(
                context.response,
                None,
                ResponseStatusCode::SUCCESS,
                create_data(),
                None,
            )
            .await;

            Ok(())
        }
    }

    let (rpc1, rpc2) = create_rpcs_with_services(
        MirrorTestService::new(),
        ResponseTestService::new(),
    );

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    let mut active_request = rpc1
        .request("test_path")
        .await
        .expect("Request failed")
        .start()
        .await
        .expect("Failed to start response");

    active_request
        .write_body_chunk(
            rmp_serde::to_vec(&create_data())
                .expect("Failed to serialize data"),
        )
        .await
        .expect("Failed to write body chunk");

    let response = active_request
        .end()
        .await
        .expect("Failed to end request")
        .wait()
        .await
        .expect("failed to wait for response");

    let (status, _, mut reader) = response.into_parts();

    let mut response_data_serialized = vec![];
    while let Some(chunk) = reader
        .read_body_chunk()
        .await
        .expect("Failed to read chunk")
    {
        response_data_serialized.extend_from_slice(&chunk);
    }

    let response_data: RpcData =
        rmp_serde::from_slice(&response_data_serialized)
            .expect("Failed to deserialize data");

    assert_eq!(status, ResponseStatusCode::SUCCESS);
    assert_eq!(response_data, create_data());

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

/// Sending a frame that references an unknown session ID must produce a
/// `ResponseError(UNEXPECTED_FRAME)` reply.  The RPC must continue
/// running and handle a subsequent valid request correctly.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_rpc_recovers_from_unknown_session_frame() {
    let (rpc, mut writer, mut reader) =
        create_rpc_with_raw_transport(MirrorTestService::new());

    let runner = {
        let rpc = rpc.clone();
        tokio::spawn(async move { rpc.run().await })
    };

    sleep(Duration::from_millis(5)).await;

    let unknown_id = Id::new();

    // Send a body-chunk frame that references a session that was never
    // started.  The RPC should reply with a ResponseError.
    write_frame(
        &mut writer,
        &Frame::request_body_chunk(unknown_id, vec![1, 2, 3]),
    )
    .await;

    // The RPC must respond with a ResponseError carrying UNEXPECTED_FRAME.
    let error_frame = read_frame(&mut reader).await;
    assert!(
        matches!(
            error_frame,
            Frame::ResponseError(ref e)
                if e.id == unknown_id
                    && e.code == ResponseErrorCode::UNEXPECTED_FRAME
        ),
        "expected ResponseError(UNEXPECTED_FRAME) for unknown id, got: \
         {error_frame:?}"
    );

    // Close cleanly.
    write_frame(&mut writer, &Frame::close()).await;
    runner
        .await
        .expect("runner task panicked")
        .expect("rpc run failed");
}

/// Sending a second `RequestStart` for the same ID while a session is
/// already open violates the protocol.  The RPC must send a
/// `ResponseError(UNEXPECTED_FRAME)`, close the bad session, and keep
/// running.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_rpc_recovers_from_out_of_order_frames() {
    // `MirrorTestService` panics when a session is forcibly closed (it
    // calls `trailers()` after seeing channel EOF, which fails because
    // `self.ended` is never set without a proper End frame).  Use
    // `NoopTestService` instead – it handles abort cleanly.
    let (rpc_reader, our_writer) = tokio::io::duplex(8192);
    let (our_reader, rpc_writer) = tokio::io::duplex(8192);
    let transport = StreamTransport::new(rpc_reader, rpc_writer);
    let rpc = BridgeRpc::new(transport, NoopTestService::new());
    let mut writer = our_writer;
    let mut reader = our_reader;

    let runner = {
        let rpc = rpc.clone();
        tokio::spawn(async move { rpc.run().await })
    };

    sleep(Duration::from_millis(5)).await;

    let id1 = Id::new();

    // Start a valid session.
    write_frame(
        &mut writer,
        &Frame::request_start(id1, "test_path".to_string(), None),
    )
    .await;

    // Send a second RequestStart for the same ID – the state machine is
    // in the Started state and does not expect another Start.
    write_frame(
        &mut writer,
        &Frame::request_start(id1, "test_path".to_string(), None),
    )
    .await;

    // The RPC must reply with a ResponseError.
    let error_frame = read_frame(&mut reader).await;
    assert!(
        matches!(
            error_frame,
            Frame::ResponseError(ref e)
                if e.id == id1
                    && e.code == ResponseErrorCode::UNEXPECTED_FRAME
        ),
        "expected ResponseError(UNEXPECTED_FRAME) for out-of-order frame, \
         got: {error_frame:?}"
    );

    // Close cleanly – the RPC must still be alive.
    write_frame(&mut writer, &Frame::close()).await;
    runner
        .await
        .expect("runner task panicked")
        .expect("rpc run failed");
}

/// After receiving (and recovering from) a bad frame the RPC must still
/// process subsequent valid requests end-to-end.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_rpc_continues_processing_valid_requests_after_bad_frame() {
    let (rpc, mut writer, mut reader) =
        create_rpc_with_raw_transport(MirrorTestService::new());

    let runner = {
        let rpc = rpc.clone();
        tokio::spawn(async move { rpc.run().await })
    };

    sleep(Duration::from_millis(5)).await;

    let unknown_id = Id::new();
    let valid_id = Id::new();
    let data = rmp_serde::to_vec(&create_data()).expect("Failed to serialize");

    // Inject a bad frame.
    write_frame(
        &mut writer,
        &Frame::request_body_chunk(unknown_id, vec![9, 8, 7]),
    )
    .await;

    // Immediately queue a valid request.
    write_frame(
        &mut writer,
        &Frame::request_start(valid_id, TEST_PATH.to_string(), None),
    )
    .await;
    write_frame(
        &mut writer,
        &Frame::request_body_chunk(valid_id, data.clone()),
    )
    .await;
    write_frame(&mut writer, &Frame::request_end(valid_id, None)).await;

    // First frame back must be the error for the bad frame.
    let first = read_frame(&mut reader).await;
    assert!(
        matches!(
            first,
            Frame::ResponseError(ref e) if e.id == unknown_id
        ),
        "expected ResponseError for unknown_id first, got: {first:?}"
    );

    // Then comes the valid response (Start → BodyChunk → End).
    let rs = read_frame(&mut reader).await;
    assert!(
        matches!(rs, Frame::ResponseStart(ref s) if s.id == valid_id),
        "expected ResponseStart for valid_id, got: {rs:?}"
    );

    let rb = read_frame(&mut reader).await;
    assert!(
        matches!(rb, Frame::ResponseBodyChunk(ref b) if b.id == valid_id && b.chunk == data),
        "expected ResponseBodyChunk with correct data, got: {rb:?}"
    );

    let re = read_frame(&mut reader).await;
    assert!(
        matches!(re, Frame::ResponseEnd(ref e) if e.id == valid_id),
        "expected ResponseEnd for valid_id, got: {re:?}"
    );

    // Close cleanly.
    write_frame(&mut writer, &Frame::close()).await;
    runner
        .await
        .expect("runner task panicked")
        .expect("rpc run failed");
}

// ──────────────────────────────────────────────────────────────────────────────
// Production-readiness integration tests
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(5000)]
async fn test_concurrent_requests() {
    let (rpc1, rpc2) = create_mirror_rpcs();
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    const N: usize = 20;

    let handles: Vec<_> = (0..N)
        .map(|i| {
            let rpc1 = rpc1.clone();
            tokio::spawn(async move {
                let message = format!("concurrent_payload_{i}");
                let data = RpcData {
                    message: message.clone(),
                };
                let serialized = rmp_serde::to_vec(&data).expect("serialize");

                let mut active = rpc1
                    .request(TEST_PATH)
                    .await
                    .expect("request")
                    .start()
                    .await
                    .expect("start");
                active
                    .write_body_chunk(serialized)
                    .await
                    .expect("write chunk");
                let response = active
                    .end()
                    .await
                    .expect("end")
                    .wait()
                    .await
                    .expect("wait");

                let (status, _, mut reader) = response.into_parts();
                assert_eq!(status, ResponseStatusCode::SUCCESS);

                let mut bytes = vec![];
                while let Some(chunk) =
                    reader.read_body_chunk().await.expect("read chunk")
                {
                    bytes.extend_from_slice(&chunk);
                }
                let received: RpcData =
                    rmp_serde::from_slice(&bytes).expect("deserialize");
                assert_eq!(
                    received.message, message,
                    "response must echo the correct request payload"
                );
            })
        })
        .collect();

    for handle in handles {
        handle.await.expect("concurrent request task panicked");
    }

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(3000)]
async fn test_bidirectional_simultaneous_requests() {
    let (rpc1, rpc2) = create_mirror_rpcs();
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    let data1 = RpcData {
        message: "from_rpc1".into(),
    };
    let data2 = RpcData {
        message: "from_rpc2".into(),
    };

    let fut1 = {
        let rpc1 = rpc1.clone();
        let data1 = data1.clone();
        async move {
            let ser = rmp_serde::to_vec(&data1).expect("serialize");
            let mut active = rpc1
                .request(TEST_PATH)
                .await
                .expect("request")
                .start()
                .await
                .expect("start");
            active.write_body_chunk(ser).await.expect("write");
            let resp =
                active.end().await.expect("end").wait().await.expect("wait");
            let (status, _, mut reader) = resp.into_parts();
            assert_eq!(status, ResponseStatusCode::SUCCESS);
            let mut bytes = vec![];
            while let Some(c) = reader.read_body_chunk().await.expect("read") {
                bytes.extend_from_slice(&c);
            }
            let received: RpcData =
                rmp_serde::from_slice(&bytes).expect("deserialize");
            assert_eq!(
                received, data1,
                "rpc1's response must echo rpc1's payload"
            );
        }
    };

    let fut2 = {
        let rpc2 = rpc2.clone();
        let data2 = data2.clone();
        async move {
            let ser = rmp_serde::to_vec(&data2).expect("serialize");
            let mut active = rpc2
                .request(TEST_PATH)
                .await
                .expect("request")
                .start()
                .await
                .expect("start");
            active.write_body_chunk(ser).await.expect("write");
            let resp =
                active.end().await.expect("end").wait().await.expect("wait");
            let (status, _, mut reader) = resp.into_parts();
            assert_eq!(status, ResponseStatusCode::SUCCESS);
            let mut bytes = vec![];
            while let Some(c) = reader.read_body_chunk().await.expect("read") {
                bytes.extend_from_slice(&c);
            }
            let received: RpcData =
                rmp_serde::from_slice(&bytes).expect("deserialize");
            assert_eq!(
                received, data2,
                "rpc2's response must echo rpc2's payload"
            );
        }
    };

    tokio::join!(fut1, fut2);

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(3000)]
async fn test_multi_chunk_body_streaming() {
    let (rpc1, rpc2) = create_mirror_rpcs();
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    const NUM_CHUNKS: usize = 5;
    const CHUNK_SIZE: usize = 128;

    // Each chunk has a distinct fill byte so we can detect corruption.
    let chunks: Vec<Vec<u8>> =
        (0..NUM_CHUNKS).map(|i| vec![i as u8; CHUNK_SIZE]).collect();

    let mut active = rpc1
        .request(TEST_PATH)
        .await
        .expect("request")
        .start()
        .await
        .expect("start");

    for chunk in &chunks {
        active
            .write_body_chunk(chunk.clone())
            .await
            .expect("write chunk");
    }

    let response = active.end().await.expect("end").wait().await.expect("wait");
    let (status, _, mut reader) = response.into_parts();
    assert_eq!(status, ResponseStatusCode::SUCCESS);

    let mut received: Vec<u8> = vec![];
    while let Some(chunk) = reader.read_body_chunk().await.expect("read chunk")
    {
        received.extend_from_slice(&chunk);
    }

    let expected: Vec<u8> = chunks.into_iter().flatten().collect();
    assert_eq!(
        received, expected,
        "all chunks must be received in full and in order"
    );

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_empty_body_request_and_response() {
    #[allow(unused)]
    #[derive(new)]
    struct EmptyBodyService {}

    #[async_trait]
    impl Service for EmptyBodyService {
        async fn run(
            &self,
            context: ServiceContext,
        ) -> Result<(), ServiceError> {
            consume_request(context.request).await;
            let response = context
                .response
                .start(ResponseStatusCode::SUCCESS)
                .await
                .expect("start response");
            response.end().await.expect("end response");
            Ok(())
        }
    }

    let (rpc1, rpc2) = create_rpcs_with_services(
        MirrorTestService::new(),
        EmptyBodyService::new(),
    );
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    // No body chunks — send Start then End immediately.
    let active = rpc1
        .request(TEST_PATH)
        .await
        .expect("request")
        .start()
        .await
        .expect("start");
    let response = active.end().await.expect("end").wait().await.expect("wait");

    let (status, _, mut reader) = response.into_parts();
    assert_eq!(status, ResponseStatusCode::SUCCESS);

    // The service sent no body chunks; read_body_chunk must return None.
    let first = reader.read_body_chunk().await.expect("read chunk");
    assert!(first.is_none(), "empty body service must return no chunks");

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(5000)]
async fn test_large_payload_round_trip() {
    let (rpc1, rpc2) = create_mirror_rpcs();
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    // 512 KB with a deterministic pattern so we can detect corruption.
    let large_data: Vec<u8> =
        (0u32..512 * 1024).map(|i| (i % 251) as u8).collect();

    let mut active = rpc1
        .request(TEST_PATH)
        .await
        .expect("request")
        .start()
        .await
        .expect("start");
    active
        .write_body_chunk(large_data.clone())
        .await
        .expect("write large chunk");
    let response = active.end().await.expect("end").wait().await.expect("wait");

    let (status, _, mut reader) = response.into_parts();
    assert_eq!(status, ResponseStatusCode::SUCCESS);

    let mut received: Vec<u8> = vec![];
    while let Some(chunk) = reader.read_body_chunk().await.expect("read chunk")
    {
        received.extend_from_slice(&chunk);
    }

    assert_eq!(
        received.len(),
        large_data.len(),
        "received byte count must match"
    );
    assert_eq!(
        received, large_data,
        "large payload must be bit-for-bit identical"
    );

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_custom_response_status_code() {
    #[allow(unused)]
    #[derive(new)]
    struct CustomStatusService {}

    #[async_trait]
    impl Service for CustomStatusService {
        async fn run(
            &self,
            context: ServiceContext,
        ) -> Result<(), ServiceError> {
            consume_request(context.request).await;
            let response = context
                .response
                .start(ResponseStatusCode::NO_HANDLER_FOR_PATH)
                .await
                .expect("start response");
            response.end().await.expect("end response");
            Ok(())
        }
    }

    let (rpc1, rpc2) = create_rpcs_with_services(
        MirrorTestService::new(),
        CustomStatusService::new(),
    );
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    let active = rpc1
        .request(TEST_PATH)
        .await
        .expect("request")
        .start()
        .await
        .expect("start");
    let response = active.end().await.expect("end").wait().await.expect("wait");

    assert_eq!(
        response.status(),
        ResponseStatusCode::NO_HANDLER_FOR_PATH,
        "non-success status code must be preserved across the wire"
    );

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(10000)]
async fn test_many_sequential_requests() {
    let (rpc1, rpc2) = create_mirror_rpcs();
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    const N: usize = 100;
    for i in 0..N {
        let message = format!("sequential_{i}");
        let data = RpcData {
            message: message.clone(),
        };
        let serialized = rmp_serde::to_vec(&data).expect("serialize");

        let mut active = rpc1
            .request(TEST_PATH)
            .await
            .expect("request")
            .start()
            .await
            .expect("start");
        active.write_body_chunk(serialized).await.expect("write");
        let response =
            active.end().await.expect("end").wait().await.expect("wait");
        let (status, _, mut reader) = response.into_parts();
        assert_eq!(
            status,
            ResponseStatusCode::SUCCESS,
            "request {i} should succeed"
        );
        let mut bytes = vec![];
        while let Some(chunk) = reader.read_body_chunk().await.expect("read") {
            bytes.extend_from_slice(&chunk);
        }
        let received: RpcData =
            rmp_serde::from_slice(&bytes).expect("deserialize");
        assert_eq!(
            received.message, message,
            "request {i} response must echo correct payload"
        );
    }

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_server_response_error_propagates_to_client() {
    #[allow(unused)]
    #[derive(new)]
    struct ErrorResponseService {}

    #[async_trait]
    impl Service for ErrorResponseService {
        async fn run(
            &self,
            context: ServiceContext,
        ) -> Result<(), ServiceError> {
            consume_request(context.request).await;
            let active = context
                .response
                .start(ResponseStatusCode::SUCCESS)
                .await
                .expect("start response");
            // Send an error frame — this consumes `active`.
            // The Drop impl will send ResponseEnd, which our new error
            // handling absorbs gracefully (state-machine warning logged,
            // session closed, run loop continues).
            active
                .error(
                    ResponseErrorCode::UNEXPECTED_FRAME,
                    "simulated server error",
                )
                .await
                .expect("send error frame");
            Ok(())
        }
    }

    let (rpc1, rpc2) = create_rpcs_with_services(
        MirrorTestService::new(),
        ErrorResponseService::new(),
    );
    let runner = run_rpcs!(rpc1, rpc2);
    sleep(Duration::from_millis(5)).await;

    let active = rpc1
        .request(TEST_PATH)
        .await
        .expect("request")
        .start()
        .await
        .expect("start");
    let response = active
        .end()
        .await
        .expect("end")
        .wait()
        .await
        .expect("wait for response start");

    let (status, _, mut reader) = response.into_parts();
    assert_eq!(status, ResponseStatusCode::SUCCESS);

    // The service sent a ResponseError; the client must surface it.
    let result = reader.read_body_chunk().await;
    assert!(
        result.is_err(),
        "read_body_chunk must return Err when the service sent a ResponseError"
    );

    close_rpcs!(rpc1, rpc2);
    runner.await.expect("runner failed");
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
struct RpcData {
    message: String,
}

type TestTransport = StreamTransport<DuplexStream, DuplexStream>;

#[derive(new)]
struct MirrorTestService {}

#[async_trait]
impl Service for MirrorTestService {
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let (_, headers, mut reader) = context.request.into_parts();

        let mut chunks = vec![];

        while let Some(chunk) = reader
            .read_body_chunk()
            .await
            .expect("Failed to read chunk")
        {
            chunks.push(chunk);
        }

        let trailers = reader
            .trailers()
            .expect("Failed to get trailers")
            .map(|t| t.clone());
        let mut response = if let Some(headers) = headers {
            context
                .response
                .start_with_headers(ResponseStatusCode::SUCCESS, headers)
                .await
                .expect("Failed to start response with headers")
        } else {
            context
                .response
                .start(ResponseStatusCode::SUCCESS)
                .await
                .expect("Failed to start response")
        };

        for chunk in chunks {
            response
                .write_body_chunk(chunk)
                .await
                .expect("Failed to write chunk");
        }

        if let Some(trailers) = trailers {
            response
                .end_with_trailers(trailers)
                .await
                .expect("Failed to end response with trailers");
        } else {
            response.end().await.expect("Failed to end response");
        }

        Ok(())
    }
}

fn create_rpcs_with_services<TService1: Service, TService2: Service>(
    tservice1: TService1,
    tservice2: TService2,
) -> (
    BridgeRpc<TestTransport, TService1>,
    BridgeRpc<TestTransport, TService2>,
) {
    let (pipe1_in, pipe1_out) = tokio::io::duplex(2048);
    let (pipe2_in, pipe2_out) = tokio::io::duplex(2048);

    let transport1 = StreamTransport::new(pipe1_in, pipe2_out);
    let transport2 = StreamTransport::new(pipe2_in, pipe1_out);

    (
        BridgeRpc::new(transport1, tservice1),
        BridgeRpc::new(transport2, tservice2),
    )
}

fn create_mirror_rpcs() -> (
    BridgeRpc<TestTransport, MirrorTestService>,
    BridgeRpc<TestTransport, MirrorTestService>,
) {
    create_rpcs_with_services(
        MirrorTestService::new(),
        MirrorTestService::new(),
    )
}

fn create_headers() -> DynMap {
    let mut headers = DynMap::new();

    headers.insert_raw("test_header_item", "value");

    headers
}

fn create_trailers() -> DynMap {
    let mut trailers = DynMap::new();

    trailers.insert_raw("test_trailer_item", "value");

    trailers
}

fn create_data() -> RpcData {
    RpcData {
        message: "test".to_string(),
    }
}

async fn consume_request(request: bridge_rpc_core::server::request::Request) {
    let mut reader = request.into_reader();

    while let Some(_) = reader
        .read_body_chunk()
        .await
        .expect("Failed to read chunk")
    {}
}

async fn write_response<TData: Serialize>(
    response: bridge_rpc_core::server::response::PendingResponse,
    headers: Option<bridge_rpc_core::DynMap>,
    status: bridge_rpc_core::ResponseStatusCode,
    data: TData,
    trailers: Option<bridge_rpc_core::DynMap>,
) {
    let mut response = if let Some(headers) = headers {
        response
            .start_with_headers(status, headers)
            .await
            .expect("Failed to start response with headers")
    } else {
        response
            .start(status)
            .await
            .expect("Failed to start response")
    };
    let serialized_data =
        rmp_serde::to_vec(&data).expect("Failed to serialize data");
    response
        .write_body_chunk(serialized_data)
        .await
        .expect("Failed to write body chunk");
    if let Some(trailers) = trailers {
        response
            .end_with_trailers(trailers)
            .await
            .expect("Failed to end response with trailers");
    } else {
        response.end().await.expect("Failed to end response");
    }
}

#[macro_export]
macro_rules! run_rpcs {
    ($($rpc:ident),+) => {{
        $(
            let $rpc = $rpc.clone();
        )+

        let mut futs = tokio::task::JoinSet::new();
        $(
            futs.spawn(async move { $rpc.run().await });
        )+

        async move {
            let results = futs.join_all().await;

            for result in results {
                result?;
            }

            Ok::<_, bridge_rpc_core::BridgeRpcError>(())
        }
    }};
}

#[macro_export]
macro_rules! close_rpcs {
    ($($rpc:ident),+) => {{
        $(
            $rpc.close().await.expect(concat!("Failed to close RPC ", stringify!($rpc)));
        )+
    }};
}
