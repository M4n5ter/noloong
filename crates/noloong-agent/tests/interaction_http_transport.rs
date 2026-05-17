#![cfg(feature = "interaction-http")]

use futures_util::{SinkExt, StreamExt};
use noloong_agent::interaction::{
    InteractionError, InteractionFuture, InteractionHttpTransportConfig, InteractionNotifier,
    JsonRpcHandler, JsonRpcHandlerOutput, JsonRpcRequest, protocol::method, serve_interaction_http,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const TOKEN: &str = "test-token";

#[tokio::test]
async fn http_post_jsonrpc_round_trips() {
    let server = spawn_server(TestHandler::default()).await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth(TOKEN)
        .json(&rpc(1, "echo", json!({"hello": "world"})))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();

    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["hello"], "world");

    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth(TOKEN)
        .body("not json")
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();

    assert!(response["id"].is_null());
    assert_eq!(response["error"]["code"], -32602);

    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth(TOKEN)
        .json(&rpc(2, "notify", json!({"dropped": true})))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["notified"], true);
}

#[tokio::test]
async fn http_auth_is_required() {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = spawn_server(TestHandler {
        calls: Arc::clone(&calls),
    })
    .await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .json(&rpc(1, "echo", json!({})))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth("wrong-token")
        .json(&rpc(2, "echo", json!({})))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let response = client
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth(TOKEN)
        .json(&rpc(3, "echo", json!({})))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn http_request_body_limit_is_enforced() {
    let server = spawn_server_with_config(
        TestHandler::default(),
        InteractionHttpTransportConfig::bearer_token(TOKEN).max_request_bytes(32),
    )
    .await;
    let response = reqwest::Client::new()
        .post(format!("{}/jsonrpc", server.base_url))
        .bearer_auth(TOKEN)
        .body(serde_json::to_string(&rpc(1, "echo", json!({"large": "payload"}))).unwrap())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn http_rejects_subscription_methods() {
    let server = spawn_server(TestHandler::default()).await;
    let client = reqwest::Client::new();

    for method in [method::EVENT_SUBSCRIBE, method::DISPLAY_SUBSCRIBE] {
        let response = client
            .post(format!("{}/jsonrpc", server.base_url))
            .bearer_auth(TOKEN)
            .json(&rpc(1, method, json!({"sessionId": "session-1"})))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(response["id"], 1);
        assert_eq!(response["error"]["code"], -32602);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("requires bidirectional transport")
        );
    }
}

#[tokio::test]
async fn websocket_auth_is_required() {
    let server = spawn_server(TestHandler::default()).await;

    assert!(connect_async(&server.websocket_url).await.is_err());

    let mut request = server.websocket_url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", "Bearer wrong-token".parse().unwrap());
    assert!(connect_async(request).await.is_err());
}

#[tokio::test]
async fn websocket_jsonrpc_round_trips() {
    let server = spawn_server(TestHandler::default()).await;
    let mut websocket = connect_websocket(&server).await;

    websocket
        .send(Message::Text(
            serde_json::to_string(&rpc(1, "echo", json!({"hello": "ws"})))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();

    let response = next_json(&mut websocket).await;
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["hello"], "ws");
}

#[tokio::test]
async fn websocket_binary_messages_are_structured_errors() {
    let server = spawn_server(TestHandler::default()).await;
    let mut websocket = connect_websocket(&server).await;

    websocket
        .send(Message::Binary(vec![0, 1, 2].into()))
        .await
        .unwrap();

    let response = next_json(&mut websocket).await;
    assert!(response["id"].is_null());
    assert_eq!(response["error"]["code"], -32602);
}

#[tokio::test]
async fn websocket_request_body_limit_is_enforced() {
    let server = spawn_server_with_config(
        TestHandler::default(),
        InteractionHttpTransportConfig::bearer_token(TOKEN).max_request_bytes(64),
    )
    .await;
    let mut websocket = connect_websocket(&server).await;
    let request =
        serde_json::to_string(&rpc(1, "echo", json!({"large": "x".repeat(256)}))).unwrap();

    websocket.send(Message::Text(request.into())).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(2), websocket.next())
        .await
        .unwrap();

    assert!(next.is_none() || matches!(next, Some(Ok(Message::Close(_))) | Some(Err(_))));
}

#[tokio::test]
async fn websocket_delivers_notifications() {
    let server = spawn_server(TestHandler::default()).await;
    let mut websocket = connect_websocket(&server).await;

    websocket
        .send(Message::Text(
            serde_json::to_string(&rpc(1, "notify", json!({"value": 42})))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();

    let mut notification = None;
    let mut response = None;
    for _ in 0..2 {
        let message = next_json(&mut websocket).await;
        if message.get("method").and_then(Value::as_str) == Some("test/event") {
            notification = Some(message);
        } else if message.get("id").and_then(Value::as_i64) == Some(1) {
            response = Some(message);
        }
    }

    assert_eq!(notification.unwrap()["params"]["value"], 42);
    assert_eq!(response.unwrap()["result"]["notified"], true);
}

#[tokio::test]
async fn websocket_shutdown_closes_socket_only() {
    let server = spawn_server(TestHandler::default()).await;
    let mut websocket = connect_websocket(&server).await;

    websocket
        .send(Message::Text(
            serde_json::to_string(&rpc(1, "shutdown", json!({})))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    let response = next_json(&mut websocket).await;
    assert_eq!(response["result"]["ok"], true);
    let closed = tokio::time::timeout(Duration::from_secs(2), websocket.next())
        .await
        .unwrap();
    assert!(closed.is_none() || matches!(closed, Some(Ok(Message::Close(_)))));

    let mut second = connect_websocket(&server).await;
    second
        .send(Message::Text(
            serde_json::to_string(&rpc(2, "echo", json!({"still": "running"})))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    let response = next_json(&mut second).await;
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["still"], "running");
}

#[derive(Clone, Default)]
struct TestHandler {
    calls: Arc<AtomicUsize>,
}

impl JsonRpcHandler for TestHandler {
    fn handle<'a>(
        &'a self,
        method: &'a str,
        params: Value,
        notifier: InteractionNotifier,
    ) -> InteractionFuture<'a, JsonRpcHandlerOutput> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match method {
                "echo" => Ok(JsonRpcHandlerOutput::result(params)),
                "notify" => {
                    notifier.notify("test/event", &params)?;
                    Ok(JsonRpcHandlerOutput::result(json!({"notified": true})))
                }
                "shutdown" => Ok(JsonRpcHandlerOutput::shutdown(json!({"ok": true}))),
                other => Err(InteractionError::method_not_found(other)),
            }
        })
    }
}

struct TestServer {
    base_url: String,
    websocket_url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_server(handler: TestHandler) -> TestServer {
    spawn_server_with_config(handler, InteractionHttpTransportConfig::bearer_token(TOKEN)).await
}

async fn spawn_server_with_config(
    handler: TestHandler,
    config: InteractionHttpTransportConfig,
) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve_interaction_http(listener, handler, config)
            .await
            .unwrap();
    });
    TestServer {
        base_url: format!("http://{address}"),
        websocket_url: format!("ws://{address}/jsonrpc/ws"),
        task,
    }
}

async fn connect_websocket(
    server: &TestServer,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = server.websocket_url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    connect_async(request).await.unwrap().0
}

async fn next_json(
    websocket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = websocket.next().await.unwrap().unwrap();
    let text = match message {
        Message::Text(text) => text,
        other => panic!("expected text websocket message, got {other:?}"),
    };
    serde_json::from_str(&text).unwrap()
}

fn rpc(id: i64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest::new(id, method, params)
}
