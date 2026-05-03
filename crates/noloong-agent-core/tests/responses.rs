use noloong_agent_core::{
    AgentEventKind, AgentMessage, AgentRuntime, CancellationToken, ContentBlock, MediaBlock,
    MediaKind, MessageRole, ModelProvider, ModelRequest, ModelStreamEvent, ResponsesApiProvider,
    ResponsesApiProviderConfig, ResponsesReasoningConfig, ResponsesReasoningEffort,
    ResponsesReasoningSummary, Result, RunReport, SseReconnectConfig, StopReason, ThinkingBlock,
    ThinkingKind, ToolCall, ToolPermissionRequirement, ToolSpec,
};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

pub mod support;

use support::{HangingServer, LiveEchoTool, MockResponse, MockServer, fast_one_retry_reconnect};

#[test]
fn reconnect_config_builder_sets_stream_reconnect() {
    let config = ResponsesApiProviderConfig::new("test-responses", "test-model")
        .stream_reconnect(SseReconnectConfig::disabled());

    assert_eq!(config.stream_reconnect, SseReconnectConfig::disabled());
}

const EMPTY_COMPLETED_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-test\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-test\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

const TEXT_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-text\"}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-text\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

const OPENROUTER_TEXT_STREAM: &str = concat!(
    "data: {\"type\":\"response.content_part.delta\",\"delta\":{\"type\":\"output_text\",\"text\":\"router\"}}\n\n",
    "data: [DONE]\n\n",
);

const TOOL_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-tool\"}}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc-1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc-1\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\"}\n\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc-1\",\"output_index\":0,\"delta\":\"\\\"noloong\\\"}\"}\n\n",
    "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc-1\",\"output_index\":0,\"item\":{\"id\":\"fc-1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\\\"noloong\\\"}\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-tool\",\"status\":\"completed\",\"output\":[{\"id\":\"fc-1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\\\"noloong\\\"}\",\"status\":\"completed\"}]}}\n\n",
);

const INTERLEAVED_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-interleaved\"}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"checking\"}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"fc-2\",\"type\":\"function_call\",\"call_id\":\"call-2\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc-2\",\"output_index\":1,\"delta\":\"not-json\"}\n\n",
    "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc-2\",\"output_index\":1}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-interleaved\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

const REASONING_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-reasoning\"}}\n\n",
    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs-1\",\"delta\":\"summary\"}\n\n",
    "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"rs-1\",\"text\":\"summary\"}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs-2\",\"type\":\"reasoning\",\"encrypted_content\":\"ciphertext\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-reasoning\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

const DUPLICATE_REASONING_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-duplicate-reasoning\"}}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs-dup\",\"type\":\"reasoning\",\"encrypted_content\":\"ciphertext\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-duplicate-reasoning\",\"status\":\"completed\",\"output\":[{\"id\":\"rs-dup\",\"type\":\"reasoning\",\"encrypted_content\":\"ciphertext\"}]}}\n\n",
);

const INCOMPLETE_STREAM: &str = "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp-incomplete\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[]}}\n\n";

const FAILED_STREAM: &str =
    "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"provider exploded\"}}\n\n";

const RUNTIME_TOOL_FIRST_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-runtime-tool\"}}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc-runtime\",\"type\":\"function_call\",\"call_id\":\"call-runtime\",\"name\":\"live_echo\",\"arguments\":\"\"}}\n\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc-runtime\",\"output_index\":0,\"delta\":\"{\\\"value\\\":\\\"noloong-response-tool\\\"}\"}\n\n",
    "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc-runtime\",\"output_index\":0,\"item\":{\"id\":\"fc-runtime\",\"type\":\"function_call\",\"call_id\":\"call-runtime\",\"name\":\"live_echo\",\"arguments\":\"{\\\"value\\\":\\\"noloong-response-tool\\\"}\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-runtime-tool\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

const RUNTIME_TOOL_SECOND_STREAM: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-runtime-final\"}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"noloong-response-final\"}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-runtime-final\",\"status\":\"completed\",\"output\":[]}}\n\n",
);

#[tokio::test]
async fn config_defaults_headers_and_request_body() -> Result<()> {
    let server = MockServer::spawn(200, "text/event-stream", EMPTY_COMPLETED_STREAM).await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .api_key("secret")
            .header("X-Test", "yes")
            .max_output_tokens(256)
            .temperature(0.2)
            .store(true)
            .reasoning(
                ResponsesReasoningConfig::new()
                    .effort(ResponsesReasoningEffort::High)
                    .summary(ResponsesReasoningSummary::Detailed),
            )
            .include_encrypted_reasoning(true)
            .function_tool_strict(true)
            .native_tool(json!({"type": "web_search_preview"}))
            .extra_body("metadata", json!({"suite": "responses"})),
    )?;

    provider
        .stream_model(
            request_with_tools(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let request = server.request();
    assert!(request.raw.starts_with("POST /responses "));
    assert_eq!(request.header("authorization"), Some("Bearer secret"));
    assert_eq!(request.header("x-test"), Some("yes"));
    let body = request.json;
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], true);
    assert_eq!(body["max_output_tokens"], 256);
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(body["reasoning"]["summary"], "detailed");
    assert_eq!(body["include"][0], "reasoning.encrypted_content");
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tools"][1]["type"], "web_search_preview");
    assert_eq!(body["metadata"]["suite"], "responses");
    Ok(())
}

#[tokio::test]
async fn payload_omits_reasoning_config_by_default() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert!(body.get("reasoning").is_none());
    assert!(body.get("include").is_none());
    assert_eq!(body["store"], false);
    Ok(())
}

#[tokio::test]
async fn payload_maps_system_to_instructions() -> Result<()> {
    let body = captured_request_body(
        ModelRequest {
            messages: vec![
                AgentMessage {
                    id: "system-1".into(),
                    role: MessageRole::System,
                    content: vec![ContentBlock::Text {
                        text: "You are concise.".into(),
                    }],
                    metadata: Map::new(),
                },
                AgentMessage::user("user-1", "hello"),
            ],
            ..simple_request()
        },
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert_eq!(body["instructions"], "You are concise.");
    assert_eq!(body["input"].as_array().unwrap().len(), 1);
    assert_eq!(body["input"][0]["role"], "user");
    Ok(())
}

#[tokio::test]
async fn payload_maps_user_and_assistant_history() -> Result<()> {
    let body = captured_request_body(
        ModelRequest {
            messages: vec![
                AgentMessage::user("user-1", "hello"),
                AgentMessage::assistant(
                    "assistant-1",
                    vec![
                        ContentBlock::Text {
                            text: "visible".into(),
                        },
                        ContentBlock::Json {
                            value: json!({"ok": true}),
                        },
                    ],
                ),
            ],
            ..simple_request()
        },
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert_eq!(body["input"][0]["type"], "message");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][1]["role"], "assistant");
    assert_eq!(body["input"][1]["content"][0]["type"], "output_text");
    assert_eq!(body["input"][1]["content"][0]["text"], "visible");
    assert_eq!(body["input"][1]["content"][1]["text"], "{\"ok\":true}");
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
                metadata: Map::new(),
            }],
            ..simple_request()
        },
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        EMPTY_COMPLETED_STREAM,
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("custom role cannot be rendered for responses api")
    );
    Ok(())
}

#[tokio::test]
async fn payload_maps_function_tools_and_tool_results() -> Result<()> {
    let body = captured_request_body(
        request_with_tool_history(),
        ResponsesApiProviderConfig::new("test-responses", "test-model").function_tool_strict(false),
    )
    .await?;

    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "lookup");
    assert_eq!(body["tools"][0]["strict"], false);
    assert!(body["tools"][0].get("permissions").is_none());
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][1]["call_id"], "call-1");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["call_id"], "call-1");
    assert_eq!(body["input"][2]["output"], "result");
    Ok(())
}

#[tokio::test]
async fn payload_maps_image_url_data_url_and_file_id() -> Result<()> {
    let mut inline = MediaBlock::inline_base64(MediaKind::Image, "aW1hZ2U=");
    inline.mime_type = Some("image/png".into());
    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Media {
                media: MediaBlock::uri(MediaKind::Image, "https://example.test/image.png"),
            },
            ContentBlock::Media { media: inline },
            ContentBlock::Media {
                media: MediaBlock::provider(MediaKind::Image, "test-responses", "file-image"),
            },
        ]),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert_eq!(body["input"][0]["content"][0]["type"], "input_image");
    assert_eq!(
        body["input"][0]["content"][0]["image_url"],
        "https://example.test/image.png"
    );
    assert_eq!(
        body["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(body["input"][0]["content"][2]["file_id"], "file-image");
    Ok(())
}

#[tokio::test]
async fn payload_maps_file_url_file_id_and_opt_in_data_url() -> Result<()> {
    let mut inline = MediaBlock::inline_base64(MediaKind::File, "ZmlsZQ==");
    inline.mime_type = Some("application/pdf".into());
    inline.name = Some("doc.pdf".into());
    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Media {
                media: MediaBlock::uri(MediaKind::File, "https://example.test/doc.pdf"),
            },
            ContentBlock::Media {
                media: MediaBlock::provider(MediaKind::File, "test-responses", "file-doc"),
            },
            ContentBlock::Media { media: inline },
        ]),
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .allow_file_data_url_input(true),
    )
    .await?;

    assert_eq!(body["input"][0]["content"][0]["type"], "input_file");
    assert_eq!(
        body["input"][0]["content"][0]["file_url"],
        "https://example.test/doc.pdf"
    );
    assert_eq!(body["input"][0]["content"][1]["file_id"], "file-doc");
    assert_eq!(
        body["input"][0]["content"][2]["file_data"],
        "data:application/pdf;base64,ZmlsZQ=="
    );
    assert_eq!(body["input"][0]["content"][2]["filename"], "doc.pdf");
    Ok(())
}

#[tokio::test]
async fn payload_rejects_unsupported_audio_video_custom_media() -> Result<()> {
    for media in [
        MediaBlock::inline_base64(MediaKind::Audio, "YXVkaW8="),
        MediaBlock::inline_base64(MediaKind::Video, "dmlkZW8="),
        MediaBlock::inline_base64(MediaKind::Custom("sensor".into()), "ZGF0YQ=="),
    ] {
        let error = stream_request(
            request_with_user_content(vec![ContentBlock::Media { media }]),
            ResponsesApiProviderConfig::new("test-responses", "test-model"),
            EMPTY_COMPLETED_STREAM,
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("not supported")
                || error.to_string().contains("custom media kind")
        );
    }
    Ok(())
}

#[tokio::test]
async fn payload_replays_responses_reasoning_with_matching_scope() -> Result<()> {
    let thinking = ThinkingBlock {
        kind: ThinkingKind::Encrypted,
        text: None,
        raw: Some(json!({
            "type": "reasoning",
            "id": "rs-1",
            "encrypted_content": "ciphertext"
        })),
        replay_descriptor: Some(json!({
            "v": 1,
            "kind": "openai_responses_reasoning_replay",
            "providerId": "test-responses",
            "model": "test-model",
            "itemId": "rs-1"
        })),
        metadata: Map::new(),
    };
    let body = captured_request_body(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![
                    ContentBlock::Thinking { thinking },
                    ContentBlock::Text {
                        text: "visible".into(),
                    },
                ],
            )],
            ..simple_request()
        },
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert_eq!(body["input"][0]["type"], "reasoning");
    assert_eq!(body["input"][0]["encrypted_content"], "ciphertext");
    assert_eq!(body["input"][1]["type"], "message");
    Ok(())
}

#[tokio::test]
async fn payload_ignores_cross_provider_reasoning_replay() -> Result<()> {
    let thinking = ThinkingBlock {
        kind: ThinkingKind::Encrypted,
        text: None,
        raw: Some(json!({
            "type": "reasoning",
            "id": "rs-1",
            "encrypted_content": "ciphertext"
        })),
        replay_descriptor: Some(json!({
            "v": 1,
            "kind": "openai_responses_reasoning_replay",
            "providerId": "other-provider",
            "model": "test-model",
            "itemId": "rs-1"
        })),
        metadata: Map::new(),
    };
    let body = captured_request_body(
        ModelRequest {
            messages: vec![AgentMessage::assistant(
                "assistant-1",
                vec![
                    ContentBlock::Thinking { thinking },
                    ContentBlock::Text {
                        text: "visible".into(),
                    },
                ],
            )],
            ..simple_request()
        },
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
    )
    .await?;

    assert_eq!(body["input"][0]["type"], "message");
    assert_eq!(body["input"][0]["content"][0]["text"], "visible");
    Ok(())
}

#[tokio::test]
async fn stream_text_delta_and_completed() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        TEXT_STREAM,
    )
    .await?;

    assert!(matches!(events[0], ModelStreamEvent::Started { .. }));
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                ModelStreamEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        "hello world"
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
async fn stream_openrouter_content_part_delta() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        OPENROUTER_TEXT_STREAM,
    )
    .await?;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::TextDelta { text } if text == "router"
        )
    }));
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop
        })
    ));
    Ok(())
}

#[tokio::test]
async fn stream_accumulates_function_call_arguments() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        TOOL_STREAM,
    )
    .await?;
    let tool_call = events.iter().find_map(|event| match event {
        ModelStreamEvent::ToolCall { tool_call } => Some(tool_call),
        _ => None,
    });

    let tool_call = tool_call.expect("tool call event");
    assert_eq!(tool_call.id, "call-1");
    assert_eq!(tool_call.name, "lookup");
    assert_eq!(tool_call.arguments["query"], "noloong");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::ToolCall { .. }))
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn stream_handles_interleaved_text_and_function_calls() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        INTERLEAVED_STREAM,
    )
    .await?;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::TextDelta { text } if text == "checking"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.id == "call-2" && tool_call.arguments == Value::String("not-json".into())
        )
    }));
    Ok(())
}

#[tokio::test]
async fn stream_reasoning_summary_and_encrypted_item() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        REASONING_STREAM,
    )
    .await?;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.kind == ThinkingKind::Summary
                    && delta.text_delta.as_deref() == Some("summary")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ThinkingDelta { delta }
                if delta.kind == ThinkingKind::Encrypted
                    && delta.raw_snapshot.as_ref().and_then(|raw| raw.get("encrypted_content"))
                        == Some(&json!("ciphertext"))
        )
    }));
    Ok(())
}

#[tokio::test]
async fn stream_deduplicates_reasoning_item_from_completed_output() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        DUPLICATE_REASONING_STREAM,
    )
    .await?;

    let encrypted_reasoning_events = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                ModelStreamEvent::ThinkingDelta { delta }
                    if delta.kind == ThinkingKind::Encrypted
                        && delta.metadata.get("itemId") == Some(&json!("rs-dup"))
            )
        })
        .count();
    assert_eq!(encrypted_reasoning_events, 1);
    Ok(())
}

#[tokio::test]
async fn stream_incomplete_maps_to_length() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        INCOMPLETE_STREAM,
    )
    .await?;

    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Length
        })
    ));
    Ok(())
}

#[tokio::test]
async fn stream_failed_reports_provider_failure() -> Result<()> {
    let events = stream_request(
        simple_request(),
        ResponsesApiProviderConfig::new("test-responses", "test-model"),
        FAILED_STREAM,
    )
    .await?;

    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Failed { error }) if error == "provider exploded"
    ));
    Ok(())
}

#[tokio::test]
async fn runtime_commits_responses_text_and_thinking() -> Result<()> {
    let report = runtime_report(REASONING_STREAM, None, 1).await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Thinking { thinking }
                    if thinking.kind == ThinkingKind::Summary
                        && thinking.text.as_deref() == Some("summary")
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn runtime_executes_responses_tool_call() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::new(200, "text/event-stream", RUNTIME_TOOL_FIRST_STREAM),
        MockResponse::new(200, "text/event-stream", RUNTIME_TOOL_SECOND_STREAM),
    ])
    .await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .with_tool(Arc::new(LiveEchoTool))
        .max_turns(2)
        .build()?;

    let report = runtime.run("use the tool").await?;

    assert!(has_tool_execution(&report, "noloong-response-tool"));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.contains("noloong-response-final")
            )
        })
    }));
    assert_eq!(
        server.requests_json()[1]["input"][2]["type"],
        "function_call_output"
    );
    Ok(())
}

#[tokio::test]
async fn http_error_reports_status_and_body_excerpt() -> Result<()> {
    let server = MockServer::spawn(429, "application/json", "{\"error\":\"rate limited\"}").await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key()
            .stream_reconnect(SseReconnectConfig::disabled()),
    )?;

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("429"));
    assert!(error.to_string().contains("rate limited"));
    Ok(())
}

#[tokio::test]
async fn reconnect_retries_pre_data_disconnect() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(200, "text/event-stream", ""),
        MockResponse::new(200, "text/event-stream", EMPTY_COMPLETED_STREAM),
    ])
    .await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(server.request_count(), 2);
    Ok(())
}

#[tokio::test]
async fn stream_timeout_retries_pre_data_idle() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::hang_after_headers(200, "text/event-stream"),
        MockResponse::new(200, "text/event-stream", EMPTY_COMPLETED_STREAM),
    ])
    .await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key()
            .stream_idle_timeout(Duration::from_millis(20))
            .stream_reconnect(fast_one_retry_reconnect()),
    )?;

    provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(server.request_count(), 2);
    Ok(())
}

#[tokio::test]
async fn reconnect_does_not_retry_after_data_frame() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(
            200,
            "text/event-stream",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-broken\"}}\n\n",
        ),
        MockResponse::new(200, "text/event-stream", EMPTY_COMPLETED_STREAM),
    ])
    .await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
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

    assert_eq!(server.request_count(), 1);
    assert!(error.to_string().contains("ended before terminal event"));
    Ok(())
}

#[tokio::test]
async fn request_timeout_applies_before_initial_response() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key()
            .request_timeout(Duration::from_millis(50)),
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

#[tokio::test]
async fn cancellation_aborts_pending_request() -> Result<()> {
    let server = HangingServer::spawn().await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key()
            .request_timeout(Duration::from_secs(5)),
    )?;
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            cancellation,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("aborted"));
    Ok(())
}

async fn captured_request_body(
    request: ModelRequest,
    config: ResponsesApiProviderConfig,
) -> Result<Value> {
    let server = MockServer::spawn(200, "text/event-stream", EMPTY_COMPLETED_STREAM).await?;
    let provider = ResponsesApiProvider::new(config.base_url(server.url()).without_api_key())?;
    provider
        .stream_model(
            request,
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;
    Ok(server.request_json())
}

async fn stream_request(
    request: ModelRequest,
    config: ResponsesApiProviderConfig,
    body: &'static str,
) -> Result<Vec<ModelStreamEvent>> {
    let server = MockServer::spawn(200, "text/event-stream", body).await?;
    let provider = ResponsesApiProvider::new(config.base_url(server.url()).without_api_key())?;
    let streamed = Arc::new(Mutex::new(Vec::new()));
    let streamed_for_sink = Arc::clone(&streamed);
    let returned = provider
        .stream_model(
            request,
            Arc::new(move |event| {
                let streamed = Arc::clone(&streamed_for_sink);
                Box::pin(async move {
                    streamed.lock().expect("stream lock poisoned").push(event);
                    Ok(())
                })
            }),
            CancellationToken::new(),
        )
        .await?;
    let streamed = streamed.lock().expect("stream lock poisoned").clone();
    assert_eq!(streamed, returned);
    Ok(returned)
}

async fn runtime_report(
    body: &'static str,
    tool: Option<Arc<dyn noloong_agent_core::ToolProvider>>,
    max_turns: u64,
) -> Result<RunReport> {
    let server = MockServer::spawn(200, "text/event-stream", body).await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let mut builder = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(max_turns);
    if let Some(tool) = tool {
        builder = builder.with_tool(tool);
    }
    builder.build()?.run("hello").await
}

fn simple_request() -> ModelRequest {
    ModelRequest {
        run_id: "run-1".into(),
        turn_id: 1,
        messages: vec![AgentMessage::user("user-1", "hello")],
        context: Map::new(),
        tools: Vec::new(),
        metadata: Map::new(),
    }
}

fn request_with_tools() -> ModelRequest {
    ModelRequest {
        tools: vec![lookup_tool()],
        ..simple_request()
    }
}

fn request_with_tool_history() -> ModelRequest {
    ModelRequest {
        messages: vec![
            AgentMessage::user("user-1", "lookup"),
            AgentMessage::assistant(
                "assistant-1",
                vec![ContentBlock::ToolCall {
                    tool_call: ToolCall {
                        id: "call-1".into(),
                        name: "lookup".into(),
                        arguments: json!({"query": "noloong"}),
                    },
                }],
            ),
            AgentMessage::tool_result(
                "tool-result-1",
                "call-1",
                "lookup",
                noloong_agent_core::ToolOutput {
                    content: vec![ContentBlock::Text {
                        text: "result".into(),
                    }],
                    details: json!({}),
                    is_error: false,
                    updates: Vec::new(),
                },
            ),
        ],
        tools: vec![lookup_tool()],
        ..simple_request()
    }
}

fn request_with_user_content(content: Vec<ContentBlock>) -> ModelRequest {
    ModelRequest {
        messages: vec![AgentMessage {
            id: "user-1".into(),
            role: MessageRole::User,
            content,
            metadata: Map::new(),
        }],
        ..simple_request()
    }
}

fn lookup_tool() -> ToolSpec {
    ToolSpec {
        name: "lookup".into(),
        description: "Looks up a value.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string"
                }
            },
            "required": ["query"]
        }),
        execution_mode: None,
        permissions: vec![ToolPermissionRequirement {
            capability: "test.lookup".into(),
            description: Some("Allows lookup test calls.".into()),
            metadata: json!({ "scope": "provider-payload-boundary" }),
        }],
    }
}

fn has_tool_execution(report: &RunReport, expected_value: &str) -> bool {
    report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { tool_call_id: _, output }
                if !output.is_error
                    && output.content.iter().any(|block| {
                        matches!(
                            block,
                            ContentBlock::Text { text } if text.contains(expected_value)
                        )
                    })
        )
    })
}
