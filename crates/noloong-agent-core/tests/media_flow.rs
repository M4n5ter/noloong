pub mod support;

use noloong_agent_core::*;
use serde_json::json;
use std::sync::Arc;
use support::core::*;
use tokio::time::Duration;

#[test]
fn thinking_type_serde_round_trips_structured_payloads() -> Result<()> {
    let event = ModelStreamEvent::ThinkingDelta {
        delta: ThinkingDelta::from_summary("visible summary")
            .with_raw(json!({ "summary": [{ "text": "visible summary" }] })),
    };

    let encoded = serde_json::to_value(&event)?;
    assert_eq!(encoded["type"], "thinking_delta");
    assert_eq!(encoded["kind"], "summary");
    assert_eq!(encoded["textDelta"], "visible summary");
    assert_eq!(
        encoded["rawSnapshot"]["summary"][0]["text"],
        "visible summary"
    );

    let decoded = serde_json::from_value::<ModelStreamEvent>(encoded)?;
    assert_eq!(decoded, event);

    let legacy = serde_json::from_value::<ModelStreamEvent>(json!({
        "type": "thinking_delta",
        "text": "legacy text"
    }))?;
    assert!(matches!(
        legacy,
        ModelStreamEvent::ThinkingDelta { delta }
            if delta.kind == ThinkingKind::Raw
                && delta.text_delta.as_deref() == Some("legacy text")
    ));

    let block = ContentBlock::Thinking {
        thinking: ThinkingBlock::from_text("raw thinking"),
    };
    let encoded_block = serde_json::to_value(&block)?;
    assert_eq!(encoded_block["type"], "thinking");
    assert_eq!(encoded_block["thinking"]["kind"], "raw");
    assert_eq!(encoded_block["thinking"]["text"], "raw thinking");
    assert_eq!(
        serde_json::from_value::<ContentBlock>(encoded_block)?,
        block
    );

    Ok(())
}

#[test]
fn media_type_serde_round_trips_provider_neutral_payloads() -> Result<()> {
    let media = MediaBlock {
        mime_type: Some("image/png".into()),
        name: Some("plot.png".into()),
        ..MediaBlock::uri(MediaKind::Image, "https://example.test/plot.png")
    };
    let block = ContentBlock::Media {
        media: media.clone(),
    };
    let encoded_block = serde_json::to_value(&block)?;

    assert_eq!(encoded_block["type"], "media");
    assert_eq!(encoded_block["media"]["kind"], "image");
    assert_eq!(encoded_block["media"]["source"]["type"], "uri");
    assert_eq!(
        encoded_block["media"]["source"]["uri"],
        "https://example.test/plot.png"
    );
    assert_eq!(encoded_block["media"]["mimeType"], "image/png");
    assert_eq!(
        serde_json::from_value::<ContentBlock>(encoded_block)?,
        block
    );

    let custom = serde_json::from_value::<ContentBlock>(json!({
        "type": "media",
        "media": {
            "kind": "spectrogram",
            "source": {
                "type": "inline",
                "data": "abc",
                "encoding": "zstd"
            },
            "mimeType": "application/octet-stream"
        }
    }))?;
    assert!(matches!(
        custom,
        ContentBlock::Media {
            media: MediaBlock {
                kind: MediaKind::Custom(kind),
                source: MediaSource::Inline {
                    encoding: MediaEncoding::Custom(encoding),
                    ..
                },
                ..
            },
        } if kind == "spectrogram" && encoding == "zstd"
    ));

    let event = ModelStreamEvent::MediaDelta {
        delta: MediaDelta::from_inline_base64_delta(MediaKind::Audio, "YWJj"),
    };
    let encoded_event = serde_json::to_value(&event)?;
    assert_eq!(encoded_event["type"], "media_delta");
    assert_eq!(encoded_event["kind"], "audio");
    assert_eq!(encoded_event["dataDelta"], "YWJj");
    assert_eq!(
        serde_json::from_value::<ModelStreamEvent>(encoded_event)?,
        event
    );

    Ok(())
}

#[tokio::test]
async fn assistant_commit_media_ordering() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(MediaOrderModel))
        .with_tool(Arc::new(DelayedTool::new(
            "lookup",
            Duration::from_millis(0),
        )))
        .max_turns(1)
        .build()?;

    let report = runtime.run("media").await?;
    let assistant = report
        .state
        .messages
        .iter()
        .find(|message| matches!(message.role, noloong_agent_core::MessageRole::Assistant))
        .expect("assistant message should be committed");

    assert!(matches!(
        assistant.content.as_slice(),
        [
            ContentBlock::Thinking { .. },
            ContentBlock::Text { text },
            ContentBlock::Media {
                media:
                    MediaBlock {
                        kind: MediaKind::Image,
                        source:
                            MediaSource::Inline {
                                data,
                                encoding: MediaEncoding::Base64
                            },
                        mime_type: Some(mime_type),
                        ..
                    },
            },
            ContentBlock::Text { text: tail },
            ContentBlock::ToolCall { tool_call },
        ] if text == "answer "
            && data == "abc123"
            && mime_type == "image/png"
            && tail == "tail"
            && tool_call.name == "lookup"
    ));
    Ok(())
}

#[tokio::test]
async fn tool_output_media_preserved() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(MediaToolModel))
        .with_tool(Arc::new(MediaTool))
        .max_turns(1)
        .build()?;

    let report = runtime.run("media tool").await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { content, .. }
                    if matches!(
                        content.first(),
                        Some(ContentBlock::Media {
                            media:
                                MediaBlock {
                                    kind: MediaKind::Image,
                                    source: MediaSource::Uri { uri },
                                    ..
                                },
                        }) if uri == "https://example.test/tool.png"
                    )
            )
        })
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionUpdate { update, .. }
                if matches!(
                    update.content.first(),
                    Some(ContentBlock::Media {
                        media:
                            MediaBlock {
                                kind: MediaKind::Audio,
                                source:
                                    MediaSource::Inline {
                                        data,
                                        encoding: MediaEncoding::Base64,
                                    },
                                ..
                            },
                    }) if data == "YXVkaW8="
                )
        )
    }));
    Ok(())
}

#[tokio::test]
async fn after_tool_hook_can_rewrite_to_media() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(MediaToolModel))
        .with_tool(Arc::new(MediaTool))
        .with_tool_hook(Arc::new(MediaRewriteHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("media tool").await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { content, .. }
                    if matches!(
                        content.first(),
                        Some(ContentBlock::Media {
                            media:
                                MediaBlock {
                                    kind: MediaKind::File,
                                    source:
                                        MediaSource::Provider {
                                            provider_id,
                                            id,
                                        },
                                    ..
                                },
                        }) if provider_id == "hook-provider" && id == "file-1"
                    )
            )
        })
    }));
    Ok(())
}
