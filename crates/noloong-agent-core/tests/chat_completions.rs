use noloong_agent_core::{
    AgentMessage, AgentRuntime, CancellationToken, ChatAudioFormat, ChatCompletionsProvider,
    ChatCompletionsProviderConfig, ChatImageDetail, ChatOutputModality, ContentBlock, MediaBlock,
    MediaEncoding, MediaKind, MediaSource, MessageRole, ModelProvider, ModelRequest,
    ModelStreamEvent, Result, StopReason, ThinkingBlock, ToolCall, ToolExecutionMode, ToolSpec,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, sleep};

pub mod support;

use support::{HangingServer, MockServer};

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
