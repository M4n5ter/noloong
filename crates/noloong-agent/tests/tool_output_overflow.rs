use noloong_agent::{
    AgentManifest, AgentSession, ApprovalPolicy, Catalog, Locale, ProductToolOutputOverflowHook,
    ToolOutputOverflowConfig,
};
use noloong_agent_core::{
    AfterToolCallContext, AgentEventKind, AgentState, BoxFuture, CancellationToken, ContentBlock,
    ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink, Result, StopReason, ToolCall,
    ToolCallHook, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn small_tool_output_stays_inline() {
    let hook = ProductToolOutputOverflowHook::new(ToolOutputOverflowConfig {
        max_inline_bytes: 1024,
        temp_dir: unique_temp_dir("small"),
        ..Default::default()
    });
    let result = hook
        .after_tool_call(context(tool_output("small")), CancellationToken::new())
        .await
        .unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn large_tool_output_is_persisted_and_rewritten() {
    let temp_dir = unique_temp_dir("large");
    let hook = ProductToolOutputOverflowHook::new(ToolOutputOverflowConfig {
        max_inline_bytes: 128,
        preview_head_bytes: 64,
        preview_tail_bytes: 64,
        temp_dir: temp_dir.clone(),
    });
    let payload = large_payload();
    let result = hook
        .after_tool_call(context(tool_output(&payload)), CancellationToken::new())
        .await
        .unwrap()
        .expect("oversized output is rewritten");

    let details = result.details.expect("rewrite details exist");
    let path = details["path"].as_str().expect("overflow path exists");
    let inline_text = content_text(&result.content.expect("rewrite content exists"));
    let stored = read_stored_output(path).await;

    assert_eq!(details["overflow"].as_bool(), Some(true));
    assert_eq!(details["inlineLimitBytes"].as_u64(), Some(128));
    assert!(Path::new(path).starts_with(&temp_dir));
    assert!(
        details["previewHead"]
            .as_str()
            .unwrap()
            .contains("large-output")
    );
    assert!(
        details["previewTail"]
            .as_str()
            .unwrap()
            .contains("large-output")
    );
    assert!(details["previewOmittedBytes"].as_u64().unwrap() > 0);
    assert_eq!(content_text(&stored.content), payload);
    assert!(inline_text.contains("Tool output was too large to inline"));
    assert!(inline_text.contains("Output preview head:"));
    assert!(inline_text.contains("Output preview tail:"));
    assert!(!inline_text.contains(&payload));
}

#[tokio::test]
async fn overflow_hook_uses_catalog_locale() {
    let hook = ProductToolOutputOverflowHook::new(ToolOutputOverflowConfig {
        max_inline_bytes: 128,
        preview_head_bytes: 64,
        preview_tail_bytes: 64,
        temp_dir: unique_temp_dir("locale"),
    })
    .with_catalog(Catalog::new(Locale::Zh));
    let result = hook
        .after_tool_call(
            context(tool_output(&large_payload())),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .expect("oversized output is rewritten");
    let inline_text = content_text(&result.content.expect("rewrite content exists"));

    assert!(inline_text.contains("工具输出过长"));
    assert!(!inline_text.contains("Tool output was too large"));
}

#[tokio::test]
async fn session_runtime_rewrites_oversized_tool_result() -> Result<()> {
    let temp_dir = unique_temp_dir("runtime");
    let manifest = AgentManifest {
        approval_policy: ApprovalPolicy::AllowAll,
        ..Default::default()
    };
    let session = AgentSession::builder()
        .with_manifest(manifest)
        .with_max_inline_tool_output_bytes(128)
        .with_tool_output_temp_dir(temp_dir.clone())
        .build();
    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(LargeToolModel::default()))
        .with_tool(Arc::new(LargeOutputTool))
        .build()?;

    let report = runtime.run("use large output tool").await?;
    let output = report
        .events
        .iter()
        .find_map(|event| match &event.kind {
            AgentEventKind::ToolExecutionCompleted {
                tool_call_id,
                output,
            } if tool_call_id == "large-call" => Some(output),
            _ => None,
        })
        .expect("tool completion event exists");
    let path = output.details["path"]
        .as_str()
        .expect("overflow path exists");
    let tool_result_text = report
        .state
        .messages
        .iter()
        .find_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult { content, .. }) => Some(content_text(content)),
            _ => None,
        })
        .expect("tool result message exists");
    let stored = read_stored_output(path).await;

    assert_eq!(output.details["overflow"].as_bool(), Some(true));
    assert!(Path::new(path).starts_with(&temp_dir));
    assert_eq!(content_text(&stored.content), large_payload());
    assert!(tool_result_text.contains("Tool output was too large to inline"));
    assert!(!tool_result_text.contains(&large_payload()));
    Ok(())
}

fn context(output: ToolOutput) -> AfterToolCallContext {
    AfterToolCallContext {
        run_id: "run/with unsafe chars".into(),
        turn_id: 7,
        tool_call: ToolCall {
            id: "call:large/output".into(),
            name: "large.output".into(),
            arguments: json!({}),
        },
        output,
        state: AgentState::default(),
    }
}

fn tool_output(text: &str) -> ToolOutput {
    ToolOutput {
        content: vec![ContentBlock::Text { text: text.into() }],
        details: json!({ "source": "test" }),
        is_error: false,
        updates: Vec::new(),
    }
}

async fn read_stored_output(path: &str) -> ToolOutput {
    let bytes = tokio::fs::read(path)
        .await
        .expect("stored output is readable");
    serde_json::from_slice(&bytes).expect("stored output is valid tool output")
}

fn content_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn large_payload() -> String {
    "large-output-".repeat(512)
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "noloong-agent-tool-output-{name}-{}-{nanos}",
        std::process::id()
    ))
}

#[derive(Default)]
struct LargeToolModel {
    calls: AtomicU64,
}

impl ModelProvider for LargeToolModel {
    fn id(&self) -> &str {
        "large-tool-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "large-tool-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "large-call".into(),
                            name: "large.output".into(),
                            arguments: json!({}),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "large-tool-2".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "done".into(),
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::Stop,
                    },
                ]
            };
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

struct LargeOutputTool;

impl ToolProvider for LargeOutputTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "large.output".into(),
            description: "Returns a large output payload.".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
            permissions: Vec::new(),
        }
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(tool_output(&large_payload()))
        })
    }
}
