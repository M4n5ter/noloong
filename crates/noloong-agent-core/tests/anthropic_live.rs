use noloong_agent_core::{
    AgentEventKind, AgentMessage, AgentRuntime, AnthropicAuthScheme, AnthropicMessagesProvider,
    AnthropicMessagesProviderConfig, ContentBlock, MediaBlock, MediaKind, MessageRole,
    ModelStreamEvent, Result, RunReport,
};
use std::{env, sync::Arc};

pub mod support;

use support::{LiveEchoTool, RED_DOT_PNG_BASE64};

#[tokio::test]
#[ignore = "optional official Anthropic diagnostic; requires NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1 and ANTHROPIC_API_KEY"]
async fn official_anthropic_messages_text_thinking_and_image() -> Result<()> {
    if skip_official_anthropic_live() {
        return Ok(());
    }

    let sentinel = "noloong-anthropic-image-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(official_anthropic_provider(
            "official-anthropic-live",
            2048,
            true,
        )?))
        .max_turns(1)
        .build()?;
    let mut image = MediaBlock::inline_base64(MediaKind::Image, RED_DOT_PNG_BASE64);
    image.mime_type = Some("image/png".into());

    let report = runtime
        .run(AgentMessage {
            id: "user-official-anthropic-image".into(),
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: format!(
                        "You will receive one tiny PNG image. Think briefly, then reply with exactly `{sentinel}` as the only visible text."
                    ),
                },
                ContentBlock::Media { media: image },
            ],
            metadata: Default::default(),
        })
        .await?;

    assert!(
        has_thinking(&report, "official-anthropic-live"),
        "official Anthropic response did not include thinking"
    );
    assert_exact_assistant_text(&report, sentinel);
    Ok(())
}

#[tokio::test]
#[ignore = "optional official Anthropic diagnostic; requires NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1 and ANTHROPIC_API_KEY"]
async fn official_anthropic_messages_tool_loop_with_thinking() -> Result<()> {
    if skip_official_anthropic_live() {
        return Ok(());
    }

    let sentinel = "noloong-anthropic-tool-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(official_anthropic_provider(
            "official-anthropic-live",
            4096,
            true,
        )?))
        .with_tool(Arc::new(LiveEchoTool))
        .max_turns(2)
        .build()?;

    let report = runtime
        .run(
            "Think briefly. Call the `live_echo` tool exactly once with value `noloong-anthropic-tool`, \
             then after the tool result reply with exactly `noloong-anthropic-tool-ok` as the only visible text.",
        )
        .await?;

    assert!(
        has_thinking(&report, "official-anthropic-live"),
        "official Anthropic tool flow did not include thinking"
    );
    assert!(
        has_tool_execution(&report, "noloong-anthropic-tool"),
        "official Anthropic tool flow did not execute the expected tool call"
    );
    assert_exact_assistant_text(&report, sentinel);
    Ok(())
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_anthropic_messages_text_compatibility() -> Result<()> {
    if skip_when_env_missing("OPENROUTER_API_KEY") {
        return Ok(());
    }

    let sentinel = "noloong-openrouter-anthropic-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_anthropic_provider(
            "openrouter-anthropic-live",
            &openrouter_anthropic_live_model(),
            768,
            false,
        )?))
        .max_turns(1)
        .build()?;

    let report = runtime
        .run(format!(
            "Reply with exactly `{sentinel}` as the only visible text."
        ))
        .await?;

    assert_exact_assistant_text(&report, sentinel);
    Ok(())
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY, a declared tool-capable Anthropic Messages model, and external OpenRouter access"]
async fn openrouter_anthropic_messages_tool_loop_when_model_declared() -> Result<()> {
    if skip_when_env_missing("OPENROUTER_API_KEY") {
        return Ok(());
    }
    let Ok(model) = env::var("NOLOONG_OPENROUTER_ANTHROPIC_TOOL_MODEL") else {
        eprintln!(
            "skipping OpenRouter Anthropic Messages tool live test; set NOLOONG_OPENROUTER_ANTHROPIC_TOOL_MODEL to a tool-capable model"
        );
        return Ok(());
    };

    let sentinel = "noloong-openrouter-anthropic-tool-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_anthropic_provider(
            "openrouter-anthropic-live",
            &model,
            1024,
            false,
        )?))
        .with_tool(Arc::new(LiveEchoTool))
        .max_turns(2)
        .build()?;

    let report = runtime
        .run(
            "Call the `live_echo` tool exactly once with value `noloong-openrouter-anthropic-tool`, \
             then after the tool result reply with exactly `noloong-openrouter-anthropic-tool-ok` as the only visible text.",
        )
        .await?;

    assert!(
        has_tool_execution(&report, "noloong-openrouter-anthropic-tool"),
        "OpenRouter Anthropic Messages tool flow did not execute the expected tool call"
    );
    assert_exact_assistant_text(&report, sentinel);
    Ok(())
}

fn official_anthropic_provider(
    id: &'static str,
    max_tokens: u64,
    thinking: bool,
) -> Result<AnthropicMessagesProvider> {
    let mut config = AnthropicMessagesProviderConfig::new(id, official_anthropic_live_model())
        .max_tokens(max_tokens);
    if thinking {
        config = config.enable_thinking(1024);
    }
    AnthropicMessagesProvider::new(config)
}

fn openrouter_anthropic_provider(
    id: &'static str,
    model: &str,
    max_tokens: u64,
    thinking: bool,
) -> Result<AnthropicMessagesProvider> {
    let mut config = AnthropicMessagesProviderConfig::new(id, model)
        .base_url("https://openrouter.ai/api")
        .api_key_env("OPENROUTER_API_KEY")
        .auth_scheme(AnthropicAuthScheme::Bearer)
        .without_anthropic_version()
        .header("X-Title", "noloong-agent-core-live-test")
        .max_tokens(max_tokens);
    if thinking {
        config = config.enable_thinking(1024);
    }
    AnthropicMessagesProvider::new(config)
}

fn official_anthropic_live_model() -> String {
    env::var("NOLOONG_ANTHROPIC_LIVE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-5".into())
}

fn openrouter_anthropic_live_model() -> String {
    env::var("NOLOONG_OPENROUTER_ANTHROPIC_LIVE_MODEL").unwrap_or_else(|_| "openrouter/free".into())
}

fn skip_when_env_missing(name: &str) -> bool {
    if env::var(name).is_ok() {
        return false;
    }
    eprintln!("skipping live test because {name} is not set");
    true
}

fn skip_official_anthropic_live() -> bool {
    if env::var("NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE").as_deref() != Ok("1") {
        eprintln!(
            "skipping official Anthropic live test; set NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1 with a valid ANTHROPIC_API_KEY to opt in"
        );
        return true;
    }
    skip_when_env_missing("ANTHROPIC_API_KEY")
}

fn has_thinking(report: &RunReport, provider_id: &str) -> bool {
    let has_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == provider_id
                && delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
        )
    });
    let has_block = report.state.messages.iter().any(|message| {
        matches!(message.role, MessageRole::Assistant)
            && message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Thinking { thinking }
                        if thinking.text.as_deref().is_some_and(|text| !text.trim().is_empty())
                )
            })
    });
    has_event || has_block
}

fn has_tool_execution(report: &RunReport, expected_value: &str) -> bool {
    report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { output, .. }
                if !output.is_error
                    && output.content.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text == expected_value)
                    })
        )
    })
}

fn assert_exact_assistant_text(report: &RunReport, sentinel: &str) {
    let visible_text = assistant_visible_text(report);
    assert_eq!(
        visible_text.trim(),
        sentinel,
        "assistant visible text did not match sentinel; visible text: {visible_text}"
    );
}

fn assistant_visible_text(report: &RunReport) -> String {
    report
        .state
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
