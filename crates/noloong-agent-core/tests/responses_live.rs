use noloong_agent_core::{
    AgentRuntime, ResponsesApiProvider, ResponsesApiProviderConfig, ResponsesReasoningConfig,
    ResponsesReasoningEffort, Result,
};
use std::{env, sync::Arc};

pub mod support;

use support::{
    LiveEchoTool, assert_assistant_text_contains, has_exact_tool_execution,
    has_visible_or_raw_thinking_event_and_block, skip_when_env_missing,
};

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY and external OpenRouter access"]
async fn openrouter_responses_free_router_text_compatibility() -> Result<()> {
    if skip_when_env_missing("OPENROUTER_API_KEY") {
        return Ok(());
    }

    let sentinel = "noloong-openrouter-responses-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_responses_provider(
            "openrouter-responses-live",
            &openrouter_responses_live_model(),
            512,
            false,
        )?))
        .max_turns(1)
        .build()?;

    let report = runtime
        .run(format!(
            "Reply with exactly `{sentinel}` as the only visible text."
        ))
        .await?;

    assert_assistant_text_contains(&report, sentinel);
    Ok(())
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY, a declared tool-capable Responses model, and external OpenRouter access"]
async fn openrouter_responses_tool_loop_when_model_declared() -> Result<()> {
    if skip_when_env_missing("OPENROUTER_API_KEY") {
        return Ok(());
    }
    let Ok(model) = env::var("NOLOONG_OPENROUTER_RESPONSES_TOOL_MODEL") else {
        eprintln!(
            "skipping OpenRouter Responses tool live test; set NOLOONG_OPENROUTER_RESPONSES_TOOL_MODEL to a tool-capable model"
        );
        return Ok(());
    };

    let sentinel = "noloong-openrouter-responses-tool-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_responses_provider(
            "openrouter-responses-live",
            &model,
            768,
            false,
        )?))
        .with_tool(Arc::new(LiveEchoTool))
        .max_turns(2)
        .build()?;

    let report = runtime
        .run(
            "Call the `live_echo` tool exactly once with value `noloong-openrouter-responses-tool`, \
             then after the tool result reply with exactly `noloong-openrouter-responses-tool-ok` as the only visible text.",
        )
        .await?;

    assert!(
        has_exact_tool_execution(&report, "noloong-openrouter-responses-tool"),
        "OpenRouter Responses tool flow did not execute the expected tool call"
    );
    assert_assistant_text_contains(&report, sentinel);
    Ok(())
}

#[tokio::test]
#[ignore = "requires OPENROUTER_API_KEY, a declared reasoning-capable Responses model, and external OpenRouter access"]
async fn openrouter_responses_reasoning_when_model_declared() -> Result<()> {
    if skip_when_env_missing("OPENROUTER_API_KEY") {
        return Ok(());
    }
    let Ok(model) = env::var("NOLOONG_OPENROUTER_RESPONSES_REASONING_MODEL") else {
        eprintln!(
            "skipping OpenRouter Responses reasoning live test; set NOLOONG_OPENROUTER_RESPONSES_REASONING_MODEL to a reasoning-capable model"
        );
        return Ok(());
    };

    let sentinel = "noloong-openrouter-responses-reasoning-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(openrouter_responses_provider(
            "openrouter-responses-live",
            &model,
            1024,
            true,
        )?))
        .max_turns(1)
        .build()?;

    let report = runtime
        .run(format!(
            "Think briefly, then reply with exactly `{sentinel}` as the only visible text."
        ))
        .await?;

    assert!(
        has_visible_or_raw_thinking_event_and_block(&report, "openrouter-responses-live"),
        "OpenRouter Responses reasoning flow did not include thinking"
    );
    assert_assistant_text_contains(&report, sentinel);
    Ok(())
}

fn openrouter_responses_provider(
    id: &'static str,
    model: &str,
    max_output_tokens: u64,
    reasoning: bool,
) -> Result<ResponsesApiProvider> {
    let mut config = ResponsesApiProviderConfig::new(id, model)
        .base_url("https://openrouter.ai/api/v1")
        .api_key_env("OPENROUTER_API_KEY")
        .header("X-Title", "noloong-agent-core-live-test")
        .max_output_tokens(max_output_tokens);
    if reasoning {
        config =
            config.reasoning(ResponsesReasoningConfig::new().effort(ResponsesReasoningEffort::Low));
    }
    ResponsesApiProvider::new(config)
}

fn openrouter_responses_live_model() -> String {
    env::var("NOLOONG_OPENROUTER_RESPONSES_LIVE_MODEL").unwrap_or_else(|_| "openrouter/free".into())
}
