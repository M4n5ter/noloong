use noloong_agent_core::{
    AgentMessage, AgentRuntime, AnthropicAuthScheme, AnthropicMessagesProvider,
    AnthropicMessagesProviderConfig, CancellationToken, ContentBlock, MediaBlock, MediaKind,
    MessageRole, ModelProvider, ModelRequest, ModelStreamEvent, Result, StopReason, ThinkingBlock,
    ToolCall, ToolExecutionMode, ToolOutput, ToolPermissionRequirement, ToolProvider, ToolRequest,
    ToolSpec,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::time::{Duration, sleep};

pub mod support;

use support::{CapturedRequest, HangingServer, MockResponse, MockServer};

#[tokio::test]
async fn config_sends_official_headers_and_defaults() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").api_key("secret-key"),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.header("x-api-key"), Some("secret-key"));
    assert_eq!(body.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(body.json["model"], "claude-test");
    assert_eq!(body.json["max_tokens"], 1024);
    assert_eq!(body.json["stream"], true);
    Ok(())
}

#[tokio::test]
async fn config_supports_bearer_auth_without_version() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .api_key("router-key")
            .auth_scheme(AnthropicAuthScheme::Bearer)
            .without_anthropic_version(),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.header("authorization"), Some("Bearer router-key"));
    assert_eq!(body.header("anthropic-version"), None);
    Ok(())
}

#[tokio::test]
async fn config_files_api_adds_beta_header() -> Result<()> {
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::provider(MediaKind::File, "anthropic", "file-123"),
        }]),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .api_key("secret-key")
            .allow_files_api_media(true),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.header("anthropic-beta"), Some("files-api-2025-04-14"));
    assert_eq!(
        body.json["messages"][0]["content"][0]["source"]["file_id"],
        "file-123"
    );
    Ok(())
}

#[tokio::test]
async fn http_error_reports_status_and_body_excerpt() -> Result<()> {
    let server = MockServer::spawn(429, "application/json", "{\"error\":\"rate\"}").await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
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
    assert!(error.contains("429"));
    assert!(error.contains("rate"));
    Ok(())
}

#[tokio::test]
async fn payload_maps_text_system_tools_and_extra_body() -> Result<()> {
    let body = captured_request_body(
        request_with_history(),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .without_api_key()
            .max_tokens(256)
            .temperature(0.2)
            .extra_body("metadata", json!({ "user_id": "user-1" })),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.json["max_tokens"], 256);
    assert_eq!(body.json["temperature"], 0.2);
    assert_eq!(body.json["metadata"]["user_id"], "user-1");
    assert_eq!(body.json["system"][0]["text"], "system prompt");
    assert_eq!(body.json["messages"][0]["role"], "user");
    assert_eq!(body.json["messages"][0]["content"][0]["text"], "hello");
    assert_eq!(body.json["messages"][1]["role"], "assistant");
    assert_eq!(body.json["messages"][1]["content"][1]["type"], "tool_use");
    assert_eq!(body.json["messages"][2]["role"], "user");
    assert_eq!(
        body.json["messages"][2]["content"][0]["type"],
        "tool_result"
    );
    assert_eq!(body.json["tools"][0]["name"], "lookup");
    assert!(body.json["tools"][0].get("permissions").is_none());
    Ok(())
}

#[tokio::test]
async fn payload_rejects_custom_roles() -> Result<()> {
    let error = stream_request(
        ModelRequest {
            messages: vec![AgentMessage {
                id: "custom-1".into(),
                role: MessageRole::Custom("developer".into()),
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                metadata: Default::default(),
            }],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("custom role cannot be rendered"));
    Ok(())
}

#[tokio::test]
async fn payload_maps_inline_and_url_images() -> Result<()> {
    let mut inline = MediaBlock::inline_base64(MediaKind::Image, "aW1hZ2U=");
    inline.mime_type = Some("image/png".into());
    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Media { media: inline },
            ContentBlock::Media {
                media: MediaBlock::uri(MediaKind::Image, "https://example.test/image.png"),
            },
        ]),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.json["messages"][0]["content"][0]["type"], "image");
    assert_eq!(
        body.json["messages"][0]["content"][0]["source"]["type"],
        "base64"
    );
    assert_eq!(
        body.json["messages"][0]["content"][1]["source"]["url"],
        "https://example.test/image.png"
    );
    Ok(())
}

#[tokio::test]
async fn payload_maps_inline_and_url_documents() -> Result<()> {
    let mut inline = MediaBlock::inline_base64(MediaKind::File, "ZG9j");
    inline.mime_type = Some("application/pdf".into());
    inline.name = Some("doc.pdf".into());
    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Media { media: inline },
            ContentBlock::Media {
                media: MediaBlock::uri(MediaKind::File, "https://example.test/doc.pdf"),
            },
        ]),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.json["messages"][0]["content"][0]["type"], "document");
    assert_eq!(body.json["messages"][0]["content"][0]["title"], "doc.pdf");
    assert_eq!(
        body.json["messages"][0]["content"][1]["source"]["url"],
        "https://example.test/doc.pdf"
    );
    Ok(())
}

#[tokio::test]
async fn payload_rejects_unsupported_audio_video_custom_media() -> Result<()> {
    for media in [
        MediaBlock::inline_base64(MediaKind::Audio, "YXVkaW8="),
        MediaBlock::uri(MediaKind::Video, "https://example.test/video.mp4"),
        MediaBlock::inline_base64(MediaKind::Custom("spectrogram".into()), "abc"),
    ] {
        let error = stream_request(
            request_with_user_content(vec![ContentBlock::Media { media }]),
            AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
            text_response("ok"),
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("not supported") || error.to_string().contains("custom")
        );
    }
    Ok(())
}

#[tokio::test]
async fn payload_sends_thinking_config_when_enabled() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .without_api_key()
            .enable_thinking(2048),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.json["thinking"]["type"], "enabled");
    assert_eq!(body.json["thinking"]["budget_tokens"], 2048);
    Ok(())
}

#[tokio::test]
async fn payload_omits_thinking_config_by_default() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await?;

    assert!(body.json.get("thinking").is_none());
    Ok(())
}

#[tokio::test]
async fn stream_text_thinking_tool_call_and_finish_reason() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-1\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-1\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"rust\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(matches!(
        events.first(),
        Some(ModelStreamEvent::Started { stream_id }) if stream_id == "msg-1"
    ));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.text_delta.as_deref() == Some("think")
                    && delta.raw_snapshot.is_none()
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.metadata.get("signature") == Some(&json!("sig-1"))
                    && delta.raw_snapshot.as_ref().is_some_and(|raw| {
                        raw["thinking"] == "think" && raw["signature"] == "sig-1"
                    })
        )
    }));
    assert!(
        events.iter().any(|event| {
            matches!(event, ModelStreamEvent::TextDelta { text } if text == "hello")
        })
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.id == "toolu-1"
                    && tool_call.name == "lookup"
                    && tool_call.arguments == json!({ "query": "rust" })
        )
    }));
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::ToolUse
        })
    ));
    Ok(())
}

#[tokio::test]
async fn stream_error_reports_provider_failure() -> Result<()> {
    let response = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}\n\n";
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(events.iter().any(|event| {
        matches!(event, ModelStreamEvent::Failed { error } if error == "overloaded")
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ModelStreamEvent::Finished { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn stream_ignores_unknown_nonfatal_events() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"unknown_event\",\"value\":true}\n\n",
        "data: {\"type\":\"ping\"}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ModelStreamEvent::TextDelta { text } if text == "ok"))
    );
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop
        })
    ));
    Ok(())
}

#[tokio::test]
async fn stream_accepts_done_sentinel_from_compatible_routes() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ModelStreamEvent::TextDelta { text } if text == "ok"))
    );
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop
        })
    ));
    Ok(())
}

#[tokio::test]
async fn stream_handles_interleaved_tool_blocks() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-a\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-b\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"a\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"b\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;
    let tool_calls = events
        .iter()
        .filter_map(|event| match event {
            ModelStreamEvent::ToolCall { tool_call } => Some(tool_call),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].id, "toolu-b");
    assert_eq!(tool_calls[0].arguments, json!({ "query": "b" }));
    assert_eq!(tool_calls[1].id, "toolu-a");
    assert_eq!(tool_calls[1].arguments, json!({ "query": "a" }));
    Ok(())
}

#[tokio::test]
async fn stream_tool_use_malformed_json_policy_falls_back_to_string() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-1\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"not-json\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.arguments == json!("not-json")
        )
    }));
    Ok(())
}

#[tokio::test]
async fn payload_replays_thinking_with_matching_scope() -> Result<()> {
    let thinking = ThinkingBlock {
        text: Some("previous thinking".into()),
        raw: Some(json!({ "thinking": "previous thinking", "signature": "sig-1" })),
        replay_descriptor: Some(json!({
            "v": 1,
            "kind": "anthropic_messages_thinking_replay",
            "providerId": "anthropic",
            "model": "claude-test",
            "signature": "sig-1"
        })),
        ..ThinkingBlock::from_text("previous thinking")
    };
    let body = captured_request_body(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![ContentBlock::Thinking { thinking }],
            )],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await?;

    assert_eq!(body.json["messages"][0]["content"][0]["type"], "thinking");
    assert_eq!(
        body.json["messages"][0]["content"][0]["thinking"],
        "previous thinking"
    );
    assert_eq!(body.json["messages"][0]["content"][0]["signature"], "sig-1");
    Ok(())
}

#[tokio::test]
async fn payload_ignores_cross_provider_thinking_replay() -> Result<()> {
    let thinking = ThinkingBlock {
        text: Some("previous thinking".into()),
        raw: Some(json!({ "thinking": "previous thinking", "signature": "sig-1" })),
        replay_descriptor: Some(json!({
            "v": 1,
            "kind": "anthropic_messages_thinking_replay",
            "providerId": "other",
            "model": "claude-test",
            "signature": "sig-1"
        })),
        ..ThinkingBlock::from_text("previous thinking")
    };
    let body = captured_request_body(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![
                    ContentBlock::Text {
                        text: "visible".into(),
                    },
                    ContentBlock::Thinking { thinking },
                ],
            )],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await?;

    assert_eq!(
        body.json["messages"][0]["content"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(body.json["messages"][0]["content"][0]["text"], "visible");
    Ok(())
}

#[tokio::test]
async fn payload_rejects_unrenderable_assistant_media() -> Result<()> {
    let error = stream_request(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![ContentBlock::Media {
                    media: MediaBlock::uri(MediaKind::Image, "https://example.test/image.png"),
                }],
            )],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("assistant media blocks cannot be rendered")
    );
    Ok(())
}

#[tokio::test]
async fn payload_rejects_tool_result_blocks_in_assistant_content() -> Result<()> {
    let error = stream_request(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![ContentBlock::ToolResult {
                    tool_call_id: "toolu-1".into(),
                    tool_name: "lookup".into(),
                    content: Vec::new(),
                    is_error: false,
                }],
            )],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("tool result blocks cannot"));
    Ok(())
}

#[tokio::test]
async fn payload_rejects_non_tool_blocks_in_tool_result_messages() -> Result<()> {
    let error = stream_request(
        ModelRequest {
            messages: vec![AgentMessage {
                id: "tool-result-1".into(),
                role: MessageRole::ToolResult,
                content: vec![ContentBlock::Text {
                    text: "unexpected".into(),
                }],
                metadata: Default::default(),
            }],
            ..simple_request()
        },
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test").without_api_key(),
        text_response("ok"),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("only tool result blocks"));
    Ok(())
}

#[tokio::test]
async fn runtime_commits_anthropic_text_and_thinking() -> Result<()> {
    let response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"answer\"}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(block, ContentBlock::Thinking { thinking } if thinking.text.as_deref() == Some("think"))
        })
    }));
    assert!(report.state.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if text == "answer"))
    }));
    Ok(())
}

#[tokio::test]
async fn runtime_executes_anthropic_tool_call() -> Result<()> {
    let first_response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-1\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"rust\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let second_response = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-2\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let server = MockServer::spawn_many(vec![
        MockResponse::new(200, "text/event-stream", first_response),
        MockResponse::new(200, "text/event-stream", second_response),
    ])
    .await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .with_tool(Arc::new(EchoTool))
        .max_turns(2)
        .build()?;

    let report = runtime.run("hello").await?;
    let request_bodies = server.requests_json();

    assert!(report.state.messages.iter().any(|message| {
        matches!(message.role, MessageRole::ToolResult)
            && message.content.iter().any(|block| {
                matches!(block, ContentBlock::ToolResult { content, .. }
                if content.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text == "rust")
                }))
            })
    }));
    assert!(report.state.messages.iter().any(|message| {
        matches!(message.role, MessageRole::Assistant)
            && message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::Text { text } if text == "done"))
    }));
    assert_eq!(request_bodies.len(), 2);
    assert!(
        request_bodies[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"][0]["type"] == "tool_result")
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_aborts_pending_anthropic_request() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
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
async fn request_timeout_applies_before_anthropic_initial_response() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-test")
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

    assert!(error.to_string().contains("timed out"));
    Ok(())
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

fn request_with_user_content(content: Vec<ContentBlock>) -> ModelRequest {
    ModelRequest {
        messages: vec![AgentMessage {
            id: "user-1".into(),
            role: MessageRole::User,
            content,
            metadata: Default::default(),
        }],
        ..simple_request()
    }
}

fn request_with_history() -> ModelRequest {
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
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                    ContentBlock::ToolCall {
                        tool_call: ToolCall {
                            id: "toolu-1".into(),
                            name: "lookup".into(),
                            arguments: json!({ "query": "rust" }),
                        },
                    },
                ],
            ),
            AgentMessage::tool_result(
                "tool-result-1",
                "toolu-1",
                "lookup",
                ToolOutput {
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
            permissions: vec![ToolPermissionRequirement {
                capability: "test.lookup".into(),
                description: Some("Allows lookup test calls.".into()),
                metadata: json!({ "scope": "provider-payload-boundary" }),
            }],
        }],
        metadata: Default::default(),
    }
}

async fn captured_request_body(
    request: ModelRequest,
    config: AnthropicMessagesProviderConfig,
    response: &'static str,
) -> Result<CapturedRequest> {
    stream_request(request, config, response).await
}

async fn stream_request(
    request: ModelRequest,
    config: AnthropicMessagesProviderConfig,
    response: &'static str,
) -> Result<CapturedRequest> {
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = AnthropicMessagesProvider::new(config.base_url(server.url()))?;

    provider
        .stream_model(
            request,
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    Ok(server.request())
}

fn text_response(text: &'static str) -> &'static str {
    match text {
        "ok" => {
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n"
        }
        _ => {
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n"
        }
    }
}

struct EchoTool;

impl ToolProvider for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "lookup".into(),
            description: "Look up a value".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
            execution_mode: None,
            permissions: Vec::new(),
        }
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> noloong_agent_core::BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            Ok(ToolOutput {
                content: vec![ContentBlock::Text {
                    text: request
                        .arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                }],
                details: request.arguments,
                is_error: false,
                updates: Vec::new(),
            })
        })
    }
}
