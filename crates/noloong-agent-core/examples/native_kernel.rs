use noloong_agent_core::{
    AgentEffect, AgentRuntime, BoxFuture, CancellationToken, ContentBlock, ContextPatch,
    ContextProvider, ContextRequest, ModelProvider, ModelRequest, ModelStreamEvent,
    ModelStreamSink, StopReason, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(EchoModel))
        .with_tool(Arc::new(EchoTool))
        .with_context_provider(Arc::new(StaticContext))
        .build()?;

    let report = runtime.run("Hello from the native kernel example").await?;
    println!("events: {}", report.events.len());
    println!("messages: {}", report.state.messages.len());
    Ok(())
}

struct EchoModel;

impl ModelProvider for EchoModel {
    fn id(&self) -> &str {
        "echo-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "native-example".into(),
                },
                ModelStreamEvent::TextDelta {
                    text: "native response".into(),
                },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::Stop,
                },
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

struct EchoTool;

impl ToolProvider for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "Echo a string".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
        }
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(ToolOutput {
                content: vec![ContentBlock::Text {
                    text: request.arguments["text"].as_str().unwrap_or("").to_string(),
                }],
                details: json!({}),
                is_error: false,
                updates: Vec::new(),
            })
        })
    }
}

struct StaticContext;

impl ContextProvider for StaticContext {
    fn id(&self) -> &str {
        "static-context"
    }

    fn prepare_context<'a>(
        &'a self,
        _request: ContextRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<AgentEffect>> {
        Box::pin(async {
            Ok(vec![AgentEffect::PatchContext {
                patch: ContextPatch::Set {
                    key: "example".into(),
                    value: json!("native"),
                },
            }])
        })
    }
}
