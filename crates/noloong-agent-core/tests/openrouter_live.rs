use noloong_agent_core::{
    AgentEventKind, AgentMessage, AgentRuntime, BoxFuture, CancellationToken,
    ChatCompletionsProvider, ChatCompletionsProviderConfig, ContentBlock, MediaBlock, MediaKind,
    MessageRole, ModelStreamEvent, Result, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_deepseek_v4_flash_official_provider_with_thinking() -> Result<()> {
    run_openrouter_deepseek_text_live().await
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_deepseek_v4_flash_official_provider_with_builtin_chat_completions() -> Result<()>
{
    run_openrouter_deepseek_tool_live().await
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_free_router_image_input() -> Result<()> {
    run_openrouter_free_router_image_live().await
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_nemotron_omni_free_image_audio_input() -> Result<()> {
    run_openrouter_nemotron_omni_image_audio_live().await
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_nemotron_omni_free_video_input() -> Result<()> {
    run_openrouter_nemotron_omni_video_live().await
}

async fn run_openrouter_deepseek_text_live() -> Result<()> {
    let runtime = openrouter_deepseek_runtime(512)?;

    let report = runtime
        .run("Think briefly, then answer exactly: noloong-live-ok")
        .await?;

    let has_thinking_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == "openrouter-deepseek-official"
                && delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
        )
    });
    let has_thinking_block = report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Thinking { thinking }
                    if thinking.text.as_deref().is_some_and(|text| !text.trim().is_empty())
            )
        })
    });
    let has_answer = report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(block, ContentBlock::Text { text } if text.contains("noloong-live-ok"))
        })
    });

    assert!(
        has_thinking_event,
        "DeepSeek official OpenRouter route did not return reasoning"
    );
    assert!(has_thinking_block, "thinking was not committed as content");
    assert!(has_answer, "model response did not include expected answer");
    Ok(())
}

async fn run_openrouter_deepseek_tool_live() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_deepseek_provider(512)?))
        .with_tool(Arc::new(LiveEchoTool))
        .max_turns(1)
        .build()?;

    let report = runtime
        .run(
            "Think briefly. Then write exactly `noloong-live-text` as visible text, \
             and call the `live_echo` tool exactly once with value `noloong-live-tool`.",
        )
        .await?;

    let has_thinking_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == "openrouter-deepseek-official"
                && delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
        )
    });
    let has_visible_text = report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.contains("noloong-live-text")
            )
        })
    });
    let has_tool_call = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ToolCall { tool_call }
            } if provider == "openrouter-deepseek-official"
                && tool_call.name == "live_echo"
                && tool_call.arguments.get("value") == Some(&json!("noloong-live-tool"))
        )
    });
    let has_tool_execution = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { tool_call_id: _, output }
                if !output.is_error
                    && output.content.iter().any(|block| {
                        matches!(
                            block,
                            ContentBlock::Text { text } if text.contains("noloong-live-tool")
                        )
                    })
        )
    });

    assert!(
        has_thinking_event,
        "DeepSeek official OpenRouter route did not return reasoning"
    );
    assert!(
        has_visible_text,
        "model response did not include visible text"
    );
    assert!(
        has_tool_call,
        "model response did not include expected tool call"
    );
    assert!(has_tool_execution, "expected tool call was not executed");
    Ok(())
}

async fn run_openrouter_free_router_image_live() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_free_router_provider(768)?))
        .max_turns(1)
        .build()?;

    let mut image = MediaBlock::inline_base64(MediaKind::Image, RED_DOT_PNG_BASE64);
    image.mime_type = Some("image/png".into());

    let report = runtime
        .run(AgentMessage {
            id: "user-openrouter-free-image".into(),
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "You will receive one tiny PNG image. Think briefly, then reply with exactly this sentinel in visible text: noloong-free-image-ok".into(),
                },
                ContentBlock::Media { media: image },
            ],
            metadata: Default::default(),
        })
        .await?;

    let visible_text = report
        .state
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        visible_text.contains("noloong-free-image-ok"),
        "OpenRouter free router image response did not include expected sentinel; visible text: {visible_text}"
    );
    Ok(())
}

async fn run_openrouter_nemotron_omni_image_audio_live() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_nemotron_omni_provider(768)?))
        .max_turns(1)
        .build()?;

    let mut image = MediaBlock::inline_base64(MediaKind::Image, RED_DOT_PNG_BASE64);
    image.mime_type = Some("image/png".into());
    let mut audio = MediaBlock::inline_base64(MediaKind::Audio, silent_wav_base64());
    audio.mime_type = Some("audio/wav".into());

    let report = runtime
        .run(AgentMessage {
            id: "user-nemotron-omni-image-audio".into(),
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "You will receive exactly two media attachments: one tiny PNG image and one silent WAV audio clip. Think briefly, then reply with exactly this sentinel in visible text: noloong-omni-image-audio-ok".into(),
                },
                ContentBlock::Media { media: image },
                ContentBlock::Media { media: audio },
            ],
            metadata: Default::default(),
        })
        .await?;

    let has_thinking_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == "openrouter-nemotron-omni-free"
                && delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
        )
    });
    let visible_text = report
        .state
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        has_thinking_event,
        "Nemotron Omni image/audio OpenRouter route did not return reasoning"
    );
    assert!(
        visible_text.contains("noloong-omni-image-audio-ok"),
        "Nemotron Omni image/audio response did not include expected sentinel; visible text: {visible_text}"
    );
    Ok(())
}

async fn run_openrouter_nemotron_omni_video_live() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_nemotron_omni_provider(768)?))
        .max_turns(1)
        .build()?;

    let video = MediaBlock::uri(
        MediaKind::Video,
        "https://www.w3schools.com/html/mov_bbb.mp4",
    );

    let report = runtime
        .run(AgentMessage {
            id: "user-nemotron-omni-video".into(),
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "You will receive one MP4 video URL. Think briefly, then reply with exactly this sentinel in visible text: noloong-omni-video-ok".into(),
                },
                ContentBlock::Media { media: video },
            ],
            metadata: Default::default(),
        })
        .await?;

    let has_thinking_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == "openrouter-nemotron-omni-free"
                && delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
        )
    });
    let visible_text = report
        .state
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        has_thinking_event,
        "Nemotron Omni video OpenRouter route did not return reasoning"
    );
    assert!(
        visible_text.contains("noloong-omni-video-ok"),
        "Nemotron Omni video response did not include expected sentinel; visible text: {visible_text}"
    );
    Ok(())
}

fn openrouter_deepseek_runtime(max_tokens: u64) -> Result<AgentRuntime> {
    AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_deepseek_provider(max_tokens)?))
        .max_turns(1)
        .build()
}

fn openrouter_deepseek_provider(max_tokens: u64) -> Result<ChatCompletionsProvider> {
    ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new(
            "openrouter-deepseek-official",
            "deepseek/deepseek-v4-flash",
        )
        .base_url("https://openrouter.ai/api/v1")
        .api_key_env("OPENROUTER_API_KEY")
        .header("X-Title", "noloong-agent-core-live-test")
        .include_usage(false)
        .temperature(0.0)
        // OpenAI Chat Completions prefers `max_completion_tokens`, but this
        // OpenRouter DeepSeek route rejects it when `require_parameters` is true.
        .extra_body("max_tokens", json!(max_tokens))
        .extra_body("reasoning", json!({ "enabled": true }))
        .extra_body("include_reasoning", json!(true))
        .extra_body(
            "provider",
            json!({
                "only": ["deepseek"],
                "allow_fallbacks": false,
                "require_parameters": true
            }),
        ),
    )
}

fn openrouter_free_router_provider(max_tokens: u64) -> Result<ChatCompletionsProvider> {
    ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("openrouter-free-router", "openrouter/free")
            .base_url("https://openrouter.ai/api/v1")
            .api_key_env("OPENROUTER_API_KEY")
            .header("X-Title", "noloong-agent-core-live-test")
            .include_usage(false)
            .temperature(0.0)
            .extra_body("max_tokens", json!(max_tokens))
            .extra_body("reasoning", json!({ "enabled": true }))
            .extra_body("include_reasoning", json!(true)),
    )
}

fn openrouter_nemotron_omni_provider(max_tokens: u64) -> Result<ChatCompletionsProvider> {
    ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new(
            "openrouter-nemotron-omni-free",
            "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
        )
        .base_url("https://openrouter.ai/api/v1")
        .api_key_env("OPENROUTER_API_KEY")
        .header("X-Title", "noloong-agent-core-live-test")
        .include_usage(false)
        .temperature(0.0)
        .extra_body("max_tokens", json!(max_tokens))
        .extra_body("reasoning", json!({ "enabled": true }))
        .extra_body("include_reasoning", json!(true))
        .extra_body(
            "provider",
            json!({
                "only": ["nvidia"],
                "allow_fallbacks": false,
                "require_parameters": true
            }),
        ),
    )
}

struct LiveEchoTool;

impl ToolProvider for LiveEchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "live_echo".into(),
            description: "Echoes a value for live model tool-call conformance tests.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "value": {
                        "type": "string"
                    }
                },
                "required": ["value"],
                "additionalProperties": false
            }),
            execution_mode: None,
        }
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            Ok(ToolOutput {
                content: vec![ContentBlock::Text {
                    text: request
                        .arguments
                        .get("value")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }],
                details: request.arguments,
                is_error: false,
                updates: Vec::new(),
            })
        })
    }
}

const RED_DOT_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

fn silent_wav_base64() -> String {
    let sample_rate = 8_000_u32;
    let samples = sample_rate / 5;
    let data_size = samples * 2;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0_u8, data_size as usize));
    base64_encode(&bytes)
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}
