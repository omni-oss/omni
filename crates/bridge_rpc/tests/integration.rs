use std::time::Duration;

use bridge_rpc::{
    BridgeRpc, BridgeRpcBuilder, RequestContext, StreamContext,
    StreamTransport, Transport,
};
use ntest::timeout;
use tokio::{io::DuplexStream, time::sleep};
use tokio_stream::StreamExt;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
struct RpcResponse<T> {
    data: T,
    message: String,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
struct RpcRequest {
    message: String,
}

type TestTransport = StreamTransport<DuplexStream, DuplexStream>;

fn create_and_mutate_rpcs<TFn1, TFn2>(
    mut fn1: TFn1,
    mut fn2: TFn2,
) -> (BridgeRpc<TestTransport>, BridgeRpc<TestTransport>)
where
    TFn1: FnMut(
        BridgeRpcBuilder<TestTransport>,
    ) -> BridgeRpcBuilder<TestTransport>,
    TFn2: FnMut(
        BridgeRpcBuilder<TestTransport>,
    ) -> BridgeRpcBuilder<TestTransport>,
{
    let (pipe1_in, pipe1_out) = tokio::io::duplex(2048);
    let (pipe2_in, pipe2_out) = tokio::io::duplex(2048);

    let transport1 = StreamTransport::new(pipe1_in, pipe2_out);
    let transport2 = StreamTransport::new(pipe2_in, pipe1_out);

    let rpc1 = BridgeRpcBuilder::new(transport1).request_handler(
        "rpc1test",
        async |request: RequestContext<RpcRequest>| {
            Ok::<_, eyre::Report>(RpcResponse {
                data: request.data,
                message: "Received data from rpc1, returning it back"
                    .to_string(),
            })
        },
    );

    let rpc2 = BridgeRpcBuilder::new(transport2).request_handler(
        "rpc2test",
        |request: RequestContext<RpcRequest>| async {
            Ok::<_, eyre::Report>(RpcResponse {
                data: request.data,
                message: "Received data from rpc2, returning it back"
                    .to_string(),
            })
        },
    );

    (
        fn1(rpc1).build().expect("should be able to build"),
        fn2(rpc2).build().expect("should be able to build"),
    )
}

fn create_rpcs() -> (BridgeRpc<impl Transport>, BridgeRpc<impl Transport>) {
    create_and_mutate_rpcs(|b| b, |b| b)
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

fn create_rpc_request() -> RpcRequest {
    RpcRequest {
        message: "test".to_string(),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(1000)]
async fn test_probe() {
    let (rpc1, rpc2) = create_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    assert!(
        rpc1.probe(Duration::from_millis(100))
            .await
            .expect("Probe failed"),
        "Probe should return true"
    );

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(1000)]
async fn test_send_and_receive_data() {
    let (rpc1, rpc2) = create_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    let result1 = rpc1
        .request::<RpcResponse<RpcRequest>, _>("rpc2test", create_rpc_request())
        .await
        .expect("Request failed");

    let result2 = rpc2
        .request::<RpcResponse<RpcRequest>, _>("rpc1test", create_rpc_request())
        .await
        .expect("Request failed");

    assert_eq!(
        result1,
        RpcResponse {
            data: create_rpc_request(),
            message: "Received data from rpc2, returning it back".to_string()
        }
    );

    assert_eq!(
        result2,
        RpcResponse {
            data: create_rpc_request(),
            message: "Received data from rpc1, returning it back".to_string()
        }
    );

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(2000)]
async fn test_stream_data() {
    let (rpc1, rpc2) = create_and_mutate_rpcs(
        |rpc1| {
            rpc1.stream_handler(
                "rpc1test-stream",
                async |mut r: StreamContext<RpcRequest, Vec<u8>>| {
                    let value = r
                        .stream
                        .next()
                        .await
                        .expect("value should exist")
                        .expect("should have data");

                    println!("Received stream data: {:?}", value);
                    assert_eq!(value, vec![1, 2, 3]);
                    Ok::<_, eyre::Report>(())
                },
            )
        },
        |rpc2| rpc2,
    );

    let runner = run_rpcs!(rpc1, rpc2);

    sleep(Duration::from_millis(1)).await;

    let stream = rpc2
        .start_stream("rpc1test-stream")
        .await
        .expect("should be able to start stream");

    stream
        .send(vec![1, 2, 3])
        .await
        .expect("should be able to send data");

    stream.end().await.expect("should be able to end stream");

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}
