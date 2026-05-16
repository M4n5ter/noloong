use noloong_agent_core::{
    AgentMessage, AgentRuntime, CancellationToken, ChatAudioFormat, ChatCompletionsProvider,
    ChatCompletionsProviderConfig, ChatImageDetail, ChatOutputModality, ContentBlock,
    HttpAuthHeader, MediaBlock, MediaEncoding, MediaKind, MediaSource, MessageRole, ModelProvider,
    ModelRequest, ModelStreamEvent, Result, SseReconnectConfig, StopReason, ThinkingBlock,
    ToolCall, ToolExecutionMode, ToolPermissionRequirement, ToolSpec,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, sleep};

pub mod support;

use support::{
    DOTTED_TEST_TOOL_NAME, HangingServer, MockResponse, MockServer, TestAuthProvider,
    dotted_tool_spec, fast_one_retry_reconnect, is_provider_safe_tool_name, unique_temp_dir,
};

#[test]
fn reconnect_config_builder_sets_stream_reconnect() {
    let config = ChatCompletionsProviderConfig::new("test-chat", "test-model")
        .stream_reconnect(SseReconnectConfig::disabled());

    assert_eq!(config.stream_reconnect, SseReconnectConfig::disabled());
}

#[tokio::test]
async fn provider_payload_blocks_are_rejected() -> Result<()> {
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url("http://127.0.0.1:9")
            .api_key("secret"),
    )?;

    let error = provider
        .stream_model(
            request_with_user_content(vec![ContentBlock::ProviderPayload {
                provider: "openai.responses".into(),
                kind: "response_item".into(),
                value: json!({"type": "reasoning"}),
            }]),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .expect_err("provider payload should fail before request");

    assert!(error.to_string().contains("provider payload"));
    Ok(())
}

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
    assert!(body["tools"][0]["function"].get("permissions").is_none());
    Ok(())
}

#[tokio::test]
async fn dotted_tool_names_are_encoded_for_provider_and_decoded_from_stream() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"host_exec_start\",\"arguments\":\"{\\\"value\\\":\\\"ok\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ),
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let events = provider
        .stream_model(
            ModelRequest {
                tools: vec![dotted_tool_spec(Some(ToolExecutionMode::Parallel))],
                ..simple_request()
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let body = server.request_json();
    assert_eq!(body["tools"][0]["function"]["name"], "host_exec_start");
    assert!(
        body["tools"][0]["function"]["name"]
            .as_str()
            .is_some_and(is_provider_safe_tool_name)
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ModelStreamEvent::ToolCall { tool_call }
                if tool_call.name == "host.exec.start"
                    && tool_call.arguments["value"] == "ok"
        )
    }));
    Ok(())
}

#[tokio::test]
async fn unknown_provider_tool_alias_reports_context() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"unexpected_alias\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ),
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    let error = provider
        .stream_model(
            ModelRequest {
                tools: vec![dotted_tool_spec(Some(ToolExecutionMode::Parallel))],
                ..simple_request()
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .expect_err("unknown provider alias should fail with context")
        .to_string();

    assert!(error.contains("unexpected_alias"));
    assert!(error.contains("test-chat"));
    assert!(error.contains("test-model"));
    assert!(error.contains("host_exec_start->host.exec.start"));
    Ok(())
}

#[tokio::test]
async fn dotted_tool_names_are_encoded_in_assistant_history() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    provider
        .stream_model(
            request_with_dotted_tool_history(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let body = server.request_json();
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["name"],
        "host_exec_start"
    );
    assert_eq!(body["messages"][2]["name"], "host_exec_start");
    assert_eq!(body["tools"][0]["function"]["name"], "host_exec_start");
    Ok(())
}

#[tokio::test]
async fn omitted_history_tool_names_are_still_encoded_for_replay() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;

    provider
        .stream_model(
            request_with_dotted_tool_history_without_tools(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let body = server.request_json();
    assert!(body.get("tools").is_none());
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["name"],
        "host_exec_start"
    );
    assert_eq!(body["messages"][2]["name"], "host_exec_start");
    Ok(())
}

#[tokio::test]
async fn auth_provider_headers_override_api_key() -> Result<()> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let auth = Arc::new(TestAuthProvider::new(
        "test-auth",
        vec![
            HttpAuthHeader::new("Authorization", "Bearer dynamic"),
            HttpAuthHeader::new("X-Auth-Provider", "yes"),
        ],
    ));
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .api_key("static-secret")
            .auth_provider(auth.clone()),
    )?;

    provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let request = server.request();
    assert_eq!(request.header("authorization"), Some("Bearer dynamic"));
    assert_eq!(request.header("x-auth-provider"), Some("yes"));
    let contexts = auth.header_contexts();
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].provider_id, "test-chat");
    assert_eq!(contexts[0].method, "POST");
    assert_eq!(contexts[0].attempt, 0);
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
async fn payload_text_only_remains_string() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"], "hello");
    Ok(())
}

#[tokio::test]
async fn payload_image_uri_content_part() -> Result<()> {
    let mut image = MediaBlock::uri(MediaKind::Image, "https://example.test/image.png");
    image.mime_type = Some("image/png".into());
    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Text {
                text: "describe".into(),
            },
            ContentBlock::Media { media: image },
        ]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .image_detail(ChatImageDetail::High),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][0]["text"], "describe");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "https://example.test/image.png"
    );
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["detail"],
        "high"
    );
    Ok(())
}

#[tokio::test]
async fn payload_image_inline_content_part() -> Result<()> {
    let mut image = MediaBlock::inline_base64(MediaKind::Image, "aW1hZ2U=");
    image.mime_type = Some("image/png".into());
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media { media: image }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(
        body["messages"][0]["content"][0]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(
        body["messages"][0]["content"][0]["image_url"]["detail"],
        "auto"
    );
    Ok(())
}

#[tokio::test]
async fn payload_system_media_rejected() -> Result<()> {
    let error = stream_request(
        ModelRequest {
            messages: vec![AgentMessage {
                id: "system-1".into(),
                role: MessageRole::System,
                content: vec![ContentBlock::Media {
                    media: MediaBlock::uri(MediaKind::Image, "https://example.test/image.png"),
                }],
                metadata: Default::default(),
            }],
            ..simple_request()
        },
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("media blocks cannot be rendered")
    );
    Ok(())
}

#[tokio::test]
async fn payload_audio_inline_wav() -> Result<()> {
    let mut audio = MediaBlock::inline_base64(MediaKind::Audio, "UklGRg==");
    audio.mime_type = Some("audio/wav".into());
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media { media: audio }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"][0]["type"], "input_audio");
    assert_eq!(
        body["messages"][0]["content"][0]["input_audio"]["data"],
        "UklGRg=="
    );
    assert_eq!(
        body["messages"][0]["content"][0]["input_audio"]["format"],
        "wav"
    );
    Ok(())
}

#[tokio::test]
async fn payload_audio_uri_rejected() -> Result<()> {
    let error = stream_request(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::uri(MediaKind::Audio, "https://example.test/audio.wav"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("audio input requires inline base64")
    );
    Ok(())
}

#[tokio::test]
async fn payload_file_provider_reference() -> Result<()> {
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::provider(MediaKind::File, "test-chat", "file-123"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"][0]["type"], "file");
    assert_eq!(
        body["messages"][0]["content"][0]["file"]["file_id"],
        "file-123"
    );
    Ok(())
}

#[tokio::test]
async fn payload_file_uri_rejected() -> Result<()> {
    let error = stream_request(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::uri(MediaKind::File, "https://example.test/file.pdf"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("file input does not support URI")
    );
    Ok(())
}

#[tokio::test]
async fn provider_materializes_local_image_file_and_video_uris() -> Result<()> {
    let dir = unique_temp_dir("chat-local-media");
    tokio::fs::create_dir_all(&dir).await?;
    let image_path = dir.join("image.png");
    let file_path = dir.join("note.txt");
    let video_path = dir.join("clip.mp4");
    tokio::fs::write(&image_path, b"image").await?;
    tokio::fs::write(&file_path, b"file").await?;
    tokio::fs::write(&video_path, b"video").await?;

    let mut image = MediaBlock::uri(MediaKind::Image, format!("file://{}", image_path.display()));
    image.mime_type = Some("image/png".into());
    let mut file = MediaBlock::uri(MediaKind::File, format!("file://{}", file_path.display()));
    file.mime_type = Some("text/plain".into());
    let mut video = MediaBlock::uri(MediaKind::Video, format!("file://{}", video_path.display()));
    video.mime_type = Some("video/mp4".into());

    let body = captured_request_body(
        request_with_user_content(vec![
            ContentBlock::Media { media: image },
            ContentBlock::Media { media: file },
            ContentBlock::Media { media: video },
        ]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(
        body["messages"][0]["content"][0]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(
        body["messages"][0]["content"][1]["file"]["file_data"],
        "ZmlsZQ=="
    );
    assert_eq!(
        body["messages"][0]["content"][1]["file"]["filename"],
        "note.txt"
    );
    assert_eq!(
        body["messages"][0]["content"][2]["video_url"]["url"],
        "data:video/mp4;base64,dmlkZW8="
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn payload_video_uri_content_part() -> Result<()> {
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::uri(MediaKind::Video, "https://example.test/video.mp4"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"][0]["type"], "video_url");
    assert_eq!(
        body["messages"][0]["content"][0]["video_url"]["url"],
        "https://example.test/video.mp4"
    );
    Ok(())
}

#[tokio::test]
async fn payload_video_inline_content_part() -> Result<()> {
    let mut video = MediaBlock::inline_base64(MediaKind::Video, "dmllbw==");
    video.mime_type = Some("video/mp4".into());
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media { media: video }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["content"][0]["type"], "video_url");
    assert_eq!(
        body["messages"][0]["content"][0]["video_url"]["url"],
        "data:video/mp4;base64,dmllbw=="
    );
    Ok(())
}

#[tokio::test]
async fn payload_provider_video_default_rejected() -> Result<()> {
    let error = stream_request(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::provider(MediaKind::Video, "test-chat", "file-123"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("provider video media requires"));
    Ok(())
}

#[tokio::test]
async fn payload_provider_video_file_mapping_when_enabled() -> Result<()> {
    let body = captured_request_body(
        request_with_user_content(vec![ContentBlock::Media {
            media: MediaBlock::provider(MediaKind::Video, "test-chat", "file-123"),
        }]),
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .allow_provider_video_file_media(true),
    )
    .await?;

    assert_eq!(
        body["messages"][0]["content"][0]["file"]["file_id"],
        "file-123"
    );
    Ok(())
}

#[tokio::test]
async fn payload_audio_output_config() -> Result<()> {
    let body = captured_request_body(
        simple_request(),
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .output_modalities([ChatOutputModality::Text])
            .enable_audio_output(ChatAudioFormat::Wav, "alloy"),
    )
    .await?;

    assert_eq!(body["modalities"], json!(["text", "audio"]));
    assert_eq!(body["audio"]["format"], "wav");
    assert_eq!(body["audio"]["voice"], "alloy");
    Ok(())
}

#[tokio::test]
async fn payload_audio_modality_requires_audio_config() -> Result<()> {
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url("http://127.0.0.1:9")
            .without_api_key()
            .output_modalities([ChatOutputModality::Audio]),
    )?;

    let error = provider
        .stream_model(
            simple_request(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("audio output modality requires output audio config")
    );
    Ok(())
}

#[tokio::test]
async fn stream_audio_delta_to_media_event() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"audio\":{\"id\":\"audio-1\",\"data\":\"abc\",\"format\":\"wav\",\"transcript\":\"hello\",\"expires_at\":123}},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"123\",\"done\":true}},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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
    let audio_events = events
        .iter()
        .filter_map(|event| match event {
            ModelStreamEvent::MediaDelta { delta } => Some(delta),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(audio_events.len(), 2);
    assert_eq!(audio_events[0].kind, MediaKind::Audio);
    assert_eq!(audio_events[0].data_delta.as_deref(), Some("abc"));
    assert_eq!(audio_events[0].mime_type.as_deref(), Some("audio/wav"));
    assert_eq!(audio_events[0].metadata["transcript"], "hello");
    assert!(matches!(
        &audio_events[0].source,
        Some(MediaSource::Provider {
            provider_id,
            id,
        }) if provider_id == "test-chat" && id == "audio-1"
    ));
    assert_eq!(audio_events[1].data_delta.as_deref(), Some("123"));
    assert!(audio_events[1].done);
    Ok(())
}

#[tokio::test]
async fn runtime_commits_streamed_audio_with_provider_source_and_data() -> Result<()> {
    let response = concat!(
        "data: {\"choices\":[{\"delta\":{\"audio\":{\"id\":\"audio-1\",\"data\":\"abc\",\"format\":\"wav\"}},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"audio\":{\"id\":\"audio-1\",\"data\":\"123\",\"done\":true}},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::spawn(200, "text/event-stream", response).await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key(),
    )?;
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(1)
        .build()?;

    let report = runtime.run("audio").await?;
    let assistant = report
        .state
        .messages
        .iter()
        .find(|message| matches!(message.role, MessageRole::Assistant))
        .expect("assistant message should be committed");
    let media = assistant
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Media { media } => Some(media),
            _ => None,
        })
        .expect("assistant message should contain streamed audio media");

    assert_eq!(media.kind, MediaKind::Audio);
    assert_eq!(media.mime_type.as_deref(), Some("audio/wav"));
    assert!(matches!(
        &media.source,
        MediaSource::Provider {
            provider_id,
            id,
        } if provider_id == "test-chat" && id == "audio-1"
    ));
    let data = media
        .data
        .as_ref()
        .expect("streamed audio data should be preserved alongside provider source");
    assert_eq!(data.data, "abc123");
    assert_eq!(data.encoding, MediaEncoding::Base64);
    Ok(())
}

#[tokio::test]
async fn payload_assistant_audio_replay() -> Result<()> {
    let body = captured_request_body(
        request_with_assistant_media(assistant_audio_replay_block(
            "test-chat",
            "test-model",
            "test-chat",
        )),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert_eq!(body["messages"][0]["role"], "assistant");
    assert_eq!(body["messages"][0]["audio"]["id"], "audio-1");
    Ok(())
}

#[tokio::test]
async fn payload_assistant_media_cross_provider_ignored() -> Result<()> {
    let body = captured_request_body(
        request_with_assistant_media(assistant_audio_replay_block(
            "other-chat",
            "test-model",
            "other-chat",
        )),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
    )
    .await?;

    assert!(body["messages"][0].get("audio").is_none());
    Ok(())
}

#[tokio::test]
async fn payload_assistant_unsupported_media_rejected() -> Result<()> {
    let error = stream_request(
        request_with_assistant_media(ContentBlock::Media {
            media: MediaBlock::uri(MediaKind::Image, "https://example.test/image.png"),
        }),
        ChatCompletionsProviderConfig::new("test-chat", "test-model"),
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
            request_with_lookup_tool(),
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
            .without_api_key()
            .stream_reconnect(SseReconnectConfig::disabled()),
    )?;

    let error = provider
        .stream_model(
            request_with_lookup_tool(),
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
async fn reconnect_retries_pre_data_disconnect() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(200, "text/event-stream", ""),
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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

    assert_eq!(server.request_count(), 2);
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop
        })
    ));
    Ok(())
}

#[tokio::test]
async fn reconnect_retries_comment_only_disconnect() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(200, "text/event-stream", ": ignored\n\n"),
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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
async fn reconnect_retries_retryable_status_before_data() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::new(500, "application/json", "{\"error\":\"temporary\"}"),
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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
async fn stream_timeout_does_not_retry_after_data_frame() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::hang_after_body(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ),
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
            .base_url(server.url())
            .without_api_key()
            .stream_idle_timeout(Duration::from_millis(20))
            .stream_reconnect(fast_one_retry_reconnect()),
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
    assert!(error.to_string().contains("stream timed out"));
    Ok(())
}

#[tokio::test]
async fn reconnect_does_not_retry_after_data_frame() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ),
        MockResponse::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
    ])
    .await?;
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

    assert_eq!(server.request_count(), 1);
    assert!(error.to_string().contains("ended before terminal event"));
    Ok(())
}

#[tokio::test]
async fn reconnect_disabled_fails_pre_data_disconnect_without_retry() -> Result<()> {
    let server = MockServer::spawn_many(vec![MockResponse::close_delimited(
        200,
        "text/event-stream",
        "",
    )])
    .await?;
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("test-chat", "test-model")
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

    assert_eq!(server.request_count(), 1);
    assert!(error.to_string().contains("after 0 reconnect attempt"));
    Ok(())
}

#[tokio::test]
async fn reconnect_exhaustion_reports_attempt_count() -> Result<()> {
    let server = MockServer::spawn_many(vec![
        MockResponse::close_delimited(200, "text/event-stream", ""),
        MockResponse::close_delimited(200, "text/event-stream", ""),
        MockResponse::close_delimited(200, "text/event-stream", ""),
    ])
    .await?;
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

    assert_eq!(server.request_count(), 3);
    let error = error.to_string();
    assert!(error.contains("chat completions stream failed"));
    assert!(error.contains("after 2 reconnect attempt"));
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
            request_with_lookup_tool(),
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
            request_with_lookup_tool(),
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
            request_with_lookup_tool(),
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

fn request_with_assistant_media(block: ContentBlock) -> ModelRequest {
    ModelRequest {
        messages: vec![AgentMessage::assistant("assistant-1", vec![block])],
        ..simple_request()
    }
}

fn assistant_audio_replay_block(
    descriptor_provider_id: &str,
    descriptor_model: &str,
    source_provider_id: &str,
) -> ContentBlock {
    let mut media = MediaBlock::provider(MediaKind::Audio, source_provider_id, "audio-1");
    media.replay_descriptor = Some(json!({
        "v": 1,
        "kind": "openai_chat_media_replay",
        "providerId": descriptor_provider_id,
        "model": descriptor_model,
        "field": "audio"
    }));
    ContentBlock::Media { media }
}

async fn captured_request_body(
    request: ModelRequest,
    config: ChatCompletionsProviderConfig,
) -> Result<Value> {
    stream_request(request, config).await
}

async fn stream_request(
    request: ModelRequest,
    config: ChatCompletionsProviderConfig,
) -> Result<Value> {
    let server = MockServer::spawn(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
    .await?;
    let provider = ChatCompletionsProvider::new(config.base_url(server.url()).without_api_key())?;

    provider
        .stream_model(
            request,
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    Ok(server.request_json())
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
        tools: vec![lookup_tool_spec()],
        metadata: Default::default(),
    }
}

fn request_with_lookup_tool() -> ModelRequest {
    ModelRequest {
        tools: vec![lookup_tool_spec()],
        ..simple_request()
    }
}

fn lookup_tool_spec() -> ToolSpec {
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
        execution_mode: Some(ToolExecutionMode::Parallel),
        permissions: vec![ToolPermissionRequirement {
            capability: "test.lookup".into(),
            description: Some("Allows lookup test calls.".into()),
            metadata: json!({ "scope": "provider-payload-boundary" }),
        }],
    }
}

fn request_with_dotted_tool_history() -> ModelRequest {
    ModelRequest {
        messages: vec![
            AgentMessage::user("user-1", "use a command"),
            AgentMessage::assistant(
                "assistant-1",
                vec![ContentBlock::ToolCall {
                    tool_call: ToolCall {
                        id: "call-1".into(),
                        name: DOTTED_TEST_TOOL_NAME.into(),
                        arguments: json!({ "value": "ok" }),
                    },
                }],
            ),
            AgentMessage::tool_result(
                "tool-result-1",
                "call-1",
                DOTTED_TEST_TOOL_NAME,
                noloong_agent_core::ToolOutput {
                    content: vec![ContentBlock::Text { text: "ok".into() }],
                    details: Value::Null,
                    is_error: false,
                    updates: Vec::new(),
                },
            ),
        ],
        tools: vec![dotted_tool_spec(Some(ToolExecutionMode::Parallel))],
        ..simple_request()
    }
}

fn request_with_dotted_tool_history_without_tools() -> ModelRequest {
    ModelRequest {
        tools: Vec::new(),
        ..request_with_dotted_tool_history()
    }
}
