#![cfg(all(feature = "interaction-client", feature = "interaction-http"))]

use noloong_agent::interaction::{
    InteractionClientError, InteractionError, InteractionFuture, InteractionHttpTransportConfig,
    InteractionNotifier, InteractionWsClient, InteractionWsClientConfig, JsonRpcHandler,
    JsonRpcHandlerOutput, serve_interaction_http,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;

const TOKEN: &str = "client-token";

#[tokio::test]
async fn client_round_trips_requests() {
    let server = spawn_server().await;
    let client = connect_client(&server.url).await;

    let response = client
        .request("echo", json!({"hello": "client"}))
        .await
        .unwrap();

    assert_eq!(response["hello"], "client");
}

#[tokio::test]
async fn client_surfaces_json_rpc_errors() {
    let server = spawn_server().await;
    let client = connect_client(&server.url).await;

    let error = client.request("fail", json!({})).await.unwrap_err();

    assert_eq!(
        error,
        InteractionClientError::JsonRpc {
            code: -32602,
            message: "bad request".into(),
            data: Some(json!({"field": "message"})),
        }
    );
}

#[tokio::test]
async fn client_delivers_notifications() {
    let server = spawn_server().await;
    let client = connect_client(&server.url).await;
    let mut notifications = client.subscribe();

    let response = client.request("notify", json!({"value": 7})).await.unwrap();
    let notification = notifications.recv().await.unwrap();

    assert_eq!(response["value"], 7);
    assert_eq!(notification.method, "display/event");
    assert_eq!(notification.params["value"], 7);
}

#[tokio::test]
async fn client_sends_bearer_auth() {
    let server = spawn_server().await;
    let client = connect_client(&server.url).await;

    let response = client.request("echo", json!({"ok": true})).await.unwrap();

    assert_eq!(response["ok"], true);
}

#[tokio::test]
async fn client_rejects_unauthorized_socket() {
    let server = spawn_server().await;
    let config =
        InteractionWsClientConfig::new(&server.url).request_timeout(Duration::from_secs(1));

    let result = InteractionWsClient::connect(config).await;

    assert!(matches!(result, Err(InteractionClientError::Connect(_))));
}

#[tokio::test]
async fn client_times_out_pending_request() {
    let server = spawn_server().await;
    let config = InteractionWsClientConfig::new(&server.url)
        .bearer_token(TOKEN)
        .request_timeout(Duration::from_millis(50));
    let client = InteractionWsClient::connect(config).await.unwrap();

    let error = client.request("ignore", json!({})).await.unwrap_err();

    assert!(matches!(error, InteractionClientError::Timeout(_)));
}

#[derive(Clone, Default)]
struct TestHandler;

impl JsonRpcHandler for TestHandler {
    fn handle<'a>(
        &'a self,
        method: &'a str,
        params: Value,
        notifier: InteractionNotifier,
    ) -> InteractionFuture<'a, JsonRpcHandlerOutput> {
        Box::pin(async move {
            match method {
                "echo" => Ok(JsonRpcHandlerOutput::result(params)),
                "fail" => Err(InteractionError::invalid_params("bad request")
                    .with_data(json!({"field": "message"}))),
                "notify" => {
                    notifier.notify("display/event", &params).await?;
                    Ok(JsonRpcHandlerOutput::result(params))
                }
                "ignore" => std::future::pending().await,
                other => Err(InteractionError::method_not_found(other)),
            }
        })
    }
}

struct TestServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_server() -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve_interaction_http(
            listener,
            TestHandler,
            InteractionHttpTransportConfig::bearer_token(TOKEN),
        )
        .await
        .unwrap();
    });
    TestServer {
        url: format!("ws://{address}/jsonrpc/ws"),
        task,
    }
}

async fn connect_client(url: &str) -> InteractionWsClient {
    InteractionWsClient::connect(
        InteractionWsClientConfig::new(url)
            .bearer_token(TOKEN)
            .request_timeout(Duration::from_secs(1)),
    )
    .await
    .unwrap()
}
