use noloong_agent_core::{
    AgentEventKind, AgentRuntime, BoxFuture, CancellationToken, ChatCompletionsProvider,
    ChatCompletionsProviderConfig, ContentBlock, ModelStreamEvent, Result, ToolOutput,
    ToolProvider, ToolRequest, ToolSpec,
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
