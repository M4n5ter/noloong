use noloong_agent::interaction::{
    INTERACTION_ERROR_INVALID_PARAMS, InteractionError, InteractionFuture, InteractionNotifier,
    JsonRpcHandler, JsonRpcHandlerOutput, serve_jsonrpc,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct TestHandler;

impl JsonRpcHandler for TestHandler {
    fn handle<'a>(
        &'a self,
        method: &'a str,
        params: Value,
        _notifier: InteractionNotifier,
    ) -> InteractionFuture<'a, JsonRpcHandlerOutput> {
        Box::pin(async move {
            match method {
                "echo" => Ok(JsonRpcHandlerOutput::result(params)),
                "shutdown" => Ok(JsonRpcHandlerOutput::shutdown(json!({"ok": true}))),
                "invalid_params" => Err(InteractionError::invalid_params("bad input")),
                other => Err(InteractionError::method_not_found(other)),
            }
        })
    }
}

#[tokio::test]
async fn interaction_jsonrpc_shutdown_returns_response_then_exits() {
    let responses = run_server_with_input(concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":{\"hello\":\"world\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"shutdown\",\"params\":{}}\n",
    ))
    .await;

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["hello"], "world");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["result"]["ok"], true);
}

#[tokio::test]
async fn interaction_jsonrpc_invalid_input_is_structured_and_non_fatal() {
    let responses = run_server_with_input(concat!(
        "not json\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"invalid_params\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"shutdown\",\"params\":{}}\n",
    ))
    .await;

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], Value::Null);
    assert_eq!(
        responses[0]["error"]["code"],
        INTERACTION_ERROR_INVALID_PARAMS
    );
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(
        responses[1]["error"]["code"],
        INTERACTION_ERROR_INVALID_PARAMS
    );
    assert_eq!(responses[2]["result"]["ok"], true);
}

#[tokio::test]
async fn interaction_jsonrpc_unknown_method_returns_method_not_found() {
    let responses = run_server_with_input(concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"missing\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"shutdown\",\"params\":{}}\n",
    ))
    .await;

    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["error"]["code"], -32601);
    assert_eq!(responses[1]["result"]["ok"], true);
}

async fn run_server_with_input(input: &str) -> Vec<Value> {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (server_reader, server_writer) = tokio::io::split(server);
    let server = tokio::spawn(serve_jsonrpc(server_reader, server_writer, TestHandler));
    let (client_reader, mut client_writer) = tokio::io::split(client);
    client_writer.write_all(input.as_bytes()).await.unwrap();
    client_writer.flush().await.unwrap();

    let mut lines = BufReader::new(client_reader).lines();
    let mut responses = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        responses.push(serde_json::from_str::<Value>(&line).unwrap());
        if responses
            .last()
            .and_then(|response| response.get("result"))
            .and_then(|result| result.get("ok"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            break;
        }
    }
    drop(client_writer);
    server.await.unwrap().unwrap();
    responses
}
