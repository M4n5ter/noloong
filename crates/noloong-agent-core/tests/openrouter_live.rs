use noloong_agent_core::{
    AgentEventKind, AgentRuntime, ContentBlock, ModelStreamEvent, Result, StdioExtensionConfig,
};
use std::{env, path::PathBuf, time::Duration};

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_deepseek_v4_flash_official_provider_with_thinking() -> Result<()> {
    let _api_key = env::var("OPENROUTER_API_KEY")
        .expect("OPENROUTER_API_KEY must be set for this live model test");
    let fixture = fixture_path("openrouter-deepseek-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .arg(fixture.to_string_lossy())
                .request_timeout(Duration::from_secs(60)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime
        .run("Think briefly, then answer exactly: noloong-live-ok")
        .await?;

    let has_thinking_event = report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { text }
            } if provider == "openrouter-deepseek-official" && !text.trim().is_empty()
        )
    });
    let has_thinking_block = report.state.messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Thinking { text } if !text.trim().is_empty()),
        )
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

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
