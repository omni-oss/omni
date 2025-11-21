use std::time::Duration;

use bridge_rpc::{
    BridgeRpc, BridgeRpcBuilder, RequestContext, StreamTransport, Transport,
};
use ntest::timeout;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
struct RpcResponse<T> {
    data: T,
    message: String,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
struct RpcRequest {
    message: String,
}

fn create_rpcs() -> (BridgeRpc<impl Transport>, BridgeRpc<impl Transport>) {
    let (pipe1_in, pipe1_out) = tokio::io::duplex(2048);
    let (pipe2_in, pipe2_out) = tokio::io::duplex(2048);

    let transport1 = StreamTransport::new(pipe1_in, pipe2_out);
    let transport2 = StreamTransport::new(pipe2_in, pipe1_out);

    let rpc1 = BridgeRpcBuilder::new(transport1)
        .request_handler(
            "rpc1test",
            |request: RequestContext<RpcRequest>| async move {
                Ok::<_, eyre::Report>(RpcResponse {
                    data: request.data,
                    message: "Received data from rpc1, returning it back"
                        .to_string(),
                })
            },
        )
        .build();

    let rpc2 = BridgeRpcBuilder::new(transport2)
        .request_handler(
            "rpc2test",
            |request: RequestContext<RpcRequest>| async move {
                Ok::<_, eyre::Report>(RpcResponse {
                    data: request.data,
                    message: "Received data from rpc2, returning it back"
                        .to_string(),
                })
            },
        )
        .build();

    (rpc1, rpc2)
}

macro_rules! run_rpcs {
    ($($rpc:ident),+) => {{
        $(
            let $rpc = $rpc.clone();
        )+
        tokio::spawn(async move {
            _ = tokio::join!($($rpc.run()),+);
        })
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

#[tokio::test]
#[timeout(1000)]
async fn test_probe() {
    let (rpc1, rpc2) = create_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);
    assert!(
        rpc1.probe(Duration::from_millis(100))
            .await
            .expect("Probe failed"),
        "Probe should return true"
    );

    close_rpcs!(rpc1, rpc2);

    runner.await.expect("Failed to run RPC");
}

#[tokio::test]
async fn test_send_and_receive_data() {
    let (rpc1, rpc2) = create_rpcs();

    let runner = run_rpcs!(rpc1, rpc2);

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
