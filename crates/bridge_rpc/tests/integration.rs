use std::time::Duration;

use async_trait::async_trait;
use bridge_rpc::{
    BridgeRpc, DynMap, ResponseStatusCode, StreamTransport,
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use derive_new::new;
use ntest::timeout;
use tokio::{io::DuplexStream, time::sleep};

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
                .start_with_headers(ResponseStatusCode::Success, headers)
                .await
                .expect("Failed to start response with headers")
        } else {
            context
                .response
                .start(ResponseStatusCode::Success)
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

            Ok::<_, bridge_rpc::BridgeRpcError>(())
        }
    }};
}

macro_rules! close_rpcs {
    ($($rpc:ident),+) => {{
        $(
            $rpc.close().await.expect(concat!("Failed to close RPC ", stringify!($rpc)));
        )+
    }};
}

fn create_headers() -> DynMap {
    let mut headers = DynMap::new();

    headers.insert("test_header_item".to_string(), "value".to_string());

    headers
}

fn create_trailers() -> DynMap {
    let mut trailers = DynMap::new();

    trailers.insert("test_trailer_item".to_string(), "value".to_string());

    trailers
}

fn create_data() -> RpcData {
    RpcData {
        message: "test".to_string(),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(1000)]
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
#[timeout(1000)]
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

    assert_eq!(status, ResponseStatusCode::Success);
    assert_eq!(Some(headers), response_headers);
    assert_eq!(Some(trailers), response_trailers);
    assert_eq!(response_data, request_data);

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}
