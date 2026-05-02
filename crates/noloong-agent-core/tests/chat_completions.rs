use noloong_agent_core::{
    AgentMessage, CancellationToken, ChatCompletionsProvider, ChatCompletionsProviderConfig,
    ContentBlock, MessageRole, ModelProvider, ModelRequest, ModelStreamEvent, Result, StopReason,
    ThinkingBlock, ToolCall, ToolExecutionMode, ToolSpec,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{Duration, sleep},
};

#[tokio::test]
async fn payload_maps_messages_tools_and_replay_descriptor() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key()
            .max_completion_tokens(128)
            .temperature(0.2)
            .extra_body("reasoning", json!({ "enabled": true })),
    )?;

    provider
        .stream_model(
            request_with_history(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let body = server.request_json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["max_completion_tokens"], 128);
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["reasoning"]["enabled"], true);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][2]["role"], "assistant");
    assert_eq!(body["messages"][2]["reasoning"], "previous reasoning");
    assert_eq!(
        body["messages"][2]["tool_calls"][0]["function"]["name"],
        "lookup"
    );
    assert_eq!(body["messages"][3]["role"], "tool");
    assert_eq!(body["messages"][3]["tool_call_id"], "call-1");
    assert_eq!(body["tools"][0]["function"]["name"], "lookup");
    Ok(())
}

#[tokio::test]
async fn payload_does_not_replay_reasoning_across_provider_scope() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("other-chat", "other-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    provider
        .stream_model(
            request_with_history(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let body = server.request_json();
    assert!(body["messages"][2].get("reasoning").is_none());
    Ok(())
}

#[tokio::test]
async fn sse_streams_text_thinking_tool_calls_and_finish_reason() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning\":\"think\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"rust\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&events);

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(move |event| {
                let received = Arc::clone(&received);
                Box::pin(async move {
                    received.lock().expect("events lock poisoned").push(event);
                    Ok(())
                })
            }),
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(returned, *events.lock().expect("events lock poisoned"));
    assert!(matches!(
        returned.first(),
        Some(ModelStreamEvent::Started { .. })
    ));
    assert!(
        returned.iter().any(|event| {
            matches!(event, ModelStreamEvent::TextDelta { text } if text == "hello")
        })
    );
    assert!(returned.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.text_delta.as_deref() == Some("think")
                    && delta.raw_snapshot.as_ref() == Some(&json!("think"))
        )
    }));
    assert!(returned.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.id == "call-1"
                    && tool_call.name == "lookup"
                    && tool_call.arguments == json!({ "query": "rust" })
        )
    }));
    assert!(matches!(
        returned.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::ToolUse
        })
    ));
    Ok(())
}

#[tokio::test]
async fn http_error_reports_status_and_body_excerpt() -> Result<()> {
    let server = MockServer::spawn(500, "application/json", "{\"error\":\"boom\"}").await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    let error = error.to_string();
    assert!(error.contains("500"));
    assert!(error.contains("boom"));
    Ok(())
}

#[tokio::test]
async fn thinking_details_preserve_raw_json_and_render_summary_delta() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning_details\":[{\"index\":0,\"summary\":[{\"text\":\"part one\"}]}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_details\":[{\"index\":0,\"summary\":[{\"text\":\"part one\"},{\"text\":\"part two\"}]}]},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let thinking = returned
        .iter()
        .filter_map(|event| match event {
            ModelStreamEvent::ThinkingDelta { delta } => Some(delta),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(thinking.len(), 2);
    assert_eq!(thinking[0].text_delta.as_deref(), Some("part one"));
    assert_eq!(thinking[1].text_delta.as_deref(), Some("\n\npart two"));
    assert_eq!(thinking[1].kind, noloong_agent_core::ThinkingKind::Summary);
    assert_eq!(
        thinking[1].raw_snapshot.as_ref(),
        Some(&json!([{ "index": 0, "summary": [{ "text": "part one" }, { "text": "part two" }] }]))
    );
    assert_eq!(
        thinking[1]
            .replay_descriptor
            .as_ref()
            .and_then(|value| value.get("field")),
        Some(&json!("reasoning_details"))
    );
    Ok(())
}

#[tokio::test]
async fn object_reasoning_preserves_raw_snapshot_and_summary_kind() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning\":{\"summary\":\"short\"}},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(returned.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.kind == noloong_agent_core::ThinkingKind::Summary
                    && delta.text_delta.as_deref() == Some("short")
                    && delta.raw_snapshot.as_ref() == Some(&json!({ "summary": "short" }))
        )
    }));
    Ok(())
}

#[tokio::test]
async fn arbitrary_object_reasoning_preserves_raw_snapshot_without_text() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning\":{\"steps\":[{\"id\":\"s1\",\"score\":0.8}]}},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(returned.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.text_delta.is_none()
                    && delta.raw_snapshot.as_ref()
                        == Some(&json!({ "steps": [{ "id": "s1", "score": 0.8 }] }))
        )
    }));
    Ok(())
}

#[tokio::test]
async fn legacy_function_call_streams_tool_use() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"function_call\":{\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\"}},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"function_call\":{\"arguments\":\"\\\"rust\\\"}\"}},\"finish_reason\":\"function_call\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(returned.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.id == "function-call-0"
                    && tool_call.name == "lookup"
                    && tool_call.arguments == json!({ "query": "rust" })
        )
    }));
    assert!(matches!(
        returned.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::ToolUse
        })
    ));
    Ok(())
}

#[tokio::test]
async fn content_filter_maps_to_error_finish_reason() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let returned = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(matches!(
        returned.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Error
        })
    ));
    Ok(())
}

#[tokio::test]
async fn cancellation_aborts_pending_request() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let cancellation = CancellationToken::new();
    let cancel = cancellation.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            cancellation,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, noloong_agent_core::AgentCoreError::Aborted));
    Ok(())
}

#[tokio::test]
async fn request_timeout_applies_before_initial_response() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key()
            .request_timeout(Duration::from_millis(20)),
    )?;

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("request timed out"));
    Ok(())
}

#[test]
fn config_carries_provider_specific_body_without_core_presets() {
    let config = ChatCompletionsProviderConfig::new("compatible-chat", "provider/model")
        .base_url("https://example.test/api/v1")
        .api_key_env("PROVIDER_API_KEY")
        .header("X-Title", "noloong-agent-core")
        .extra_body("reasoning", json!({ "enabled": true }))
        .extra_body("include_reasoning", json!(true))
        .extra_body(
            "provider",
            json!({
                "only": ["provider-name"],
                "allow_fallbacks": false,
                "require_parameters": true
            }),
        );

    assert_eq!(config.base_url, "https://example.test/api/v1");
    assert_eq!(config.model, "provider/model");
    assert_eq!(config.api_key_env.as_deref(), Some("PROVIDER_API_KEY"));
    assert_eq!(config.headers["X-Title"], "noloong-agent-core");
    assert_eq!(config.extra_body["reasoning"]["enabled"], true);
    assert_eq!(config.extra_body["include_reasoning"], true);
    assert_eq!(
        config.extra_body["provider"]["only"],
        json!(["provider-name"])
    );
    assert_eq!(config.extra_body["provider"]["allow_fallbacks"], false);
    assert_eq!(config.extra_body["provider"]["require_parameters"], true);
}

fn simple_request() -> ModelRequest {
    ModelRequest {
        run_id: "run-test".into(),
        turn_id: 1,
        messages: vec![AgentMessage::user("user-1", "hello")],
        context: Default::default(),
        tools: Vec::new(),
        metadata: Default::default(),
    }
}

fn request_with_history() -> ModelRequest {
    let thinking = ThinkingBlock {
        text: Some("previous reasoning".into()),
        raw: Some(json!("previous reasoning")),
        replay_descriptor: Some(json!({
            "v": 1,
            "kind": "openai_chat_reasoning_replay",
            "providerId": "test-chat",
            "model": "test-model",
            "field": "reasoning"
        })),
        ..ThinkingBlock::from_text("previous reasoning")
    };
    ModelRequest {
        run_id: "run-test".into(),
        turn_id: 1,
        messages: vec![
            AgentMessage {
                id: "system-1".into(),
                role: MessageRole::System,
                content: vec![ContentBlock::Text {
                    text: "system prompt".into(),
                }],
                metadata: Default::default(),
            },
            AgentMessage::user("user-1", "hello"),
            AgentMessage::assistant(
                "assistant-1",
                vec![
                    ContentBlock::Thinking { thinking },
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                    ContentBlock::ToolCall {
                        tool_call: ToolCall {
                            id: "call-1".into(),
                            name: "lookup".into(),
                            arguments: json!({ "query": "rust" }),
                        },
                    },
                ],
            ),
            AgentMessage::tool_result(
                "tool-result-1",
                "call-1",
                "lookup",
                noloong_agent_core::ToolOutput {
                    content: vec![ContentBlock::Text {
                        text: "result".into(),
                    }],
                    details: Value::Null,
                    is_error: false,
                    updates: Vec::new(),
                },
            ),
        ],
        context: Default::default(),
        tools: vec![ToolSpec {
            name: "lookup".into(),
            description: "Look up a value".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
            execution_mode: Some(ToolExecutionMode::Parallel),
        }],
        metadata: Default::default(),
    }
}

struct MockServer {
    address: String,
    request: Arc<Mutex<Option<String>>>,
}

impl MockServer {
    async fn spawn(status: u16, content_type: &'static str, body: &'static str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?.to_string();
        let request = Arc::new(Mutex::new(None));
        let request_slot = Arc::clone(&request);
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("mock server accept failed");
            let request = read_http_request(&mut socket)
                .await
                .expect("mock server read failed");
            *request_slot.lock().expect("request lock poisoned") = Some(request);
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("mock server write failed");
        });
        Ok(Self { address, request })
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn request_json(&self) -> Value {
        let request = self
            .request
            .lock()
            .expect("request lock poisoned")
            .clone()
            .expect("request was not received");
        let body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("request body separator");
        serde_json::from_str(body).expect("request body is valid JSON")
    }
}

struct HangingServer {
    address: String,
}

impl HangingServer {
    async fn spawn() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?.to_string();
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("mock server accept failed");
            sleep(Duration::from_secs(5)).await;
        });
        Ok(Self { address })
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

async fn read_http_request(socket: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Ok(String::from_utf8_lossy(&buffer).to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let total_len = header_end + 4 + content_length;
    while buffer.len() < total_len {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
