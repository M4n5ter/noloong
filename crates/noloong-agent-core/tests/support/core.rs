use noloong_agent_core::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Barrier;
use tokio::time::{Duration, sleep};

pub fn native_runtime() -> noloong_agent_core::AgentRuntimeBuilder {
    AgentRuntime::builder()
        .with_model_provider(Arc::new(NativeModel {
            calls: AtomicU64::new(0),
        }))
        .with_tool(Arc::new(NativeTool))
        .with_context_provider(Arc::new(NativeContext))
        .max_turns(4)
}

pub fn approval_runtime(
    store: Arc<dyn EventStore>,
    fast_calls: Arc<AtomicU64>,
    expires_at_ms: Option<u64>,
) -> Result<AgentRuntime> {
    AgentRuntime::builder()
        .with_event_store(store)
        .with_model_provider(Arc::new(FastToolModel))
        .with_tool(Arc::new(PermissionedCountingTool::new("fast", fast_calls)))
        .with_tool_hook(Arc::new(ApprovalToolHook { expires_at_ms }))
        .max_turns(1)
        .build()
}

pub struct NativeModel {
    pub calls: AtomicU64,
}

impl ModelProvider for NativeModel {
    fn id(&self) -> &str {
        "native-model"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                assert_eq!(request.context.get("native"), Some(&json!("context")));
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "native-stream-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "call-1".into(),
                            name: "native_echo".into(),
                            arguments: json!({ "text": "from model" }),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                assert!(request.messages.iter().any(|message| {
                    matches!(&message.role, noloong_agent_core::MessageRole::ToolResult)
                }));
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "native-stream-2".into(),
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

pub struct NativeTool;

impl ToolProvider for NativeTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "native_echo".into(),
            description: "Echo text from the native test tool".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
            execution_mode: None,
            permissions: Vec::new(),
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
                    text: request.arguments["text"].as_str().unwrap_or("").to_string(),
                }],
                details: json!({ "tool": "native" }),
                is_error: false,
                updates: vec![ToolUpdate {
                    content: vec![ContentBlock::Text {
                        text: "running".into(),
                    }],
                    details: json!({ "step": 1 }),
                }],
            })
        })
    }
}

pub struct NativeContext;

impl ContextProvider for NativeContext {
    fn id(&self) -> &str {
        "native-context"
    }

    fn prepare_context<'a>(
        &'a self,
        _request: ContextRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<AgentEffect>> {
        Box::pin(async move {
            Ok(vec![AgentEffect::PatchContext {
                patch: ContextPatch::Set {
                    key: "native".into(),
                    value: json!("context"),
                },
            }])
        })
    }
}

pub struct InsertedPhase;

impl PhaseNode for InsertedPhase {
    fn id(&self) -> &str {
        "test.inserted"
    }

    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async move {
            let mut output = PhaseOutput::from_scratch(context.scratch);
            output.effects.push(AgentEffect::PatchContext {
                patch: ContextPatch::Set {
                    key: "inserted".into(),
                    value: json!(true),
                },
            });
            Ok(output)
        })
    }
}

pub struct InvalidEffectPhase;

impl PhaseNode for InvalidEffectPhase {
    fn id(&self) -> &str {
        "test.invalid-effect"
    }

    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async move {
            let mut output = PhaseOutput::from_scratch(context.scratch);
            output.effects.push(AgentEffect::AppendMessage {
                message: AgentMessage::user("", "invalid"),
            });
            Ok(output)
        })
    }
}

pub struct FailingModel;

impl ModelProvider for FailingModel {
    fn id(&self) -> &str {
        "failing-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let events = vec![ModelStreamEvent::Failed {
                error: "model failed".into(),
            }];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

pub struct MediaOrderModel;

impl ModelProvider for MediaOrderModel {
    fn id(&self) -> &str {
        "media-order-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let mut image_start = MediaDelta::from_inline_base64_delta(MediaKind::Image, "abc");
            image_start.mime_type = Some("image/png".into());
            image_start.name = Some("plot.png".into());
            let mut image_end = MediaDelta::from_inline_base64_delta(MediaKind::Image, "123");
            image_end.done = true;
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "media-order".into(),
                },
                ModelStreamEvent::ThinkingDelta {
                    delta: ThinkingDelta::from_text("think"),
                },
                ModelStreamEvent::TextDelta {
                    text: "answer ".into(),
                },
                ModelStreamEvent::MediaDelta { delta: image_start },
                ModelStreamEvent::MediaDelta { delta: image_end },
                ModelStreamEvent::TextDelta {
                    text: "tail".into(),
                },
                ModelStreamEvent::ToolCall {
                    tool_call: ToolCall {
                        id: "lookup-call".into(),
                        name: "lookup".into(),
                        arguments: json!({}),
                    },
                },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::ToolUse,
                },
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

pub struct MediaToolModel;

impl ModelProvider for MediaToolModel {
    fn id(&self) -> &str {
        "media-tool-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "media-tool".into(),
                },
                ModelStreamEvent::ToolCall {
                    tool_call: ToolCall {
                        id: "media-call".into(),
                        name: "media_tool".into(),
                        arguments: json!({}),
                    },
                },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::ToolUse,
                },
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

pub struct MediaTool;

impl ToolProvider for MediaTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "media_tool".into(),
            description: "Return media content".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
            permissions: Vec::new(),
        }
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async {
            Ok(ToolOutput {
                content: vec![ContentBlock::Media {
                    media: MediaBlock::uri(MediaKind::Image, "https://example.test/tool.png"),
                }],
                details: json!({}),
                is_error: false,
                updates: vec![ToolUpdate {
                    content: vec![ContentBlock::Media {
                        media: MediaBlock::inline_base64(MediaKind::Audio, "YXVkaW8="),
                    }],
                    details: json!({ "step": "media" }),
                }],
            })
        })
    }
}

pub struct FailingContext;

impl ContextProvider for FailingContext {
    fn id(&self) -> &str {
        "failing-context"
    }

    fn prepare_context<'a>(
        &'a self,
        _request: ContextRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<AgentEffect>> {
        Box::pin(async { Err(AgentCoreError::Phase("context failed".into())) })
    }
}

pub struct FailingPhase;

impl PhaseNode for FailingPhase {
    fn id(&self) -> &str {
        "test.failing-phase"
    }

    fn run<'a>(&'a self, _context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async { Err(AgentCoreError::Phase("phase failed".into())) })
    }
}

pub struct TwoToolModel;

pub fn two_tool_events(stream_id: impl Into<String>) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::Started {
            stream_id: stream_id.into(),
        },
        ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: "slow-call".into(),
                name: "slow".into(),
                arguments: json!({}),
            },
        },
        ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: "fast-call".into(),
                name: "fast".into(),
                arguments: json!({}),
            },
        },
        ModelStreamEvent::Finished {
            stop_reason: StopReason::ToolUse,
        },
    ]
}

impl ModelProvider for TwoToolModel {
    fn id(&self) -> &str {
        "two-tool-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let events = two_tool_events("two-tools");
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

pub struct FastToolModel;

impl ModelProvider for FastToolModel {
    fn id(&self) -> &str {
        "fast-tool-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "fast-tool".into(),
                },
                ModelStreamEvent::ToolCall {
                    tool_call: ToolCall {
                        id: "fast-call".into(),
                        name: "fast".into(),
                        arguments: json!({}),
                    },
                },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::ToolUse,
                },
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

pub struct DelayedTool {
    name: &'static str,
    delay: Duration,
    execution_mode: Option<ToolExecutionMode>,
}

pub struct PermissionedCountingTool {
    name: &'static str,
    calls: Arc<AtomicU64>,
}

impl PermissionedCountingTool {
    pub fn new(name: &'static str, calls: Arc<AtomicU64>) -> Self {
        Self { name, calls }
    }
}

pub struct FailingTool(pub &'static str);

impl ToolProvider for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.0.into(),
            description: "Failing test tool".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
            permissions: Vec::new(),
        }
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async { Err(AgentCoreError::Phase("tool failed".into())) })
    }
}

impl DelayedTool {
    pub fn new(name: &'static str, delay: Duration) -> Self {
        Self {
            name,
            delay,
            execution_mode: None,
        }
    }

    pub fn new_with_mode(
        name: &'static str,
        delay: Duration,
        execution_mode: Option<ToolExecutionMode>,
    ) -> Self {
        Self {
            name,
            delay,
            execution_mode,
        }
    }
}

impl ToolProvider for DelayedTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: "Delayed test tool".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: self.execution_mode,
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
            sleep(self.delay).await;
            cancellation.throw_if_cancelled()?;
            Ok(ToolOutput {
                content: vec![ContentBlock::Text {
                    text: self.name.into(),
                }],
                details: json!({}),
                is_error: false,
                updates: Vec::new(),
            })
        })
    }
}

impl ToolProvider for PermissionedCountingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: "Permissioned counting test tool".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
            permissions: vec![ToolPermissionRequirement {
                capability: format!("test.{}", self.name),
                description: Some("Required by permission tests".into()),
                metadata: json!({ "tool": self.name }),
            }],
        }
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                content: vec![ContentBlock::Text {
                    text: self.name.into(),
                }],
                details: json!({}),
                is_error: false,
                updates: Vec::new(),
            })
        })
    }
}

pub struct TestToolHook;

impl ToolCallHook for TestToolHook {
    fn id(&self) -> Option<&str> {
        Some("test-tool-hook")
    }

    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            Ok(
                (context.tool_call.name == "slow").then_some(BeforeToolCallResult::decision(
                    ToolPermissionDecision {
                        outcome: ToolPermissionOutcome::Deny,
                        reason: Some("blocked by test hook".into()),
                        approver: Some("test".into()),
                        metadata: json!({ "source": "test" }),
                    },
                )),
            )
        })
    }

    fn after_tool_call<'a>(
        &'a self,
        context: AfterToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterToolCallResult>> {
        Box::pin(async move {
            Ok(
                (context.tool_call.name == "fast").then_some(AfterToolCallResult {
                    content: Some(vec![ContentBlock::Text {
                        text: "rewritten".into(),
                    }]),
                    details: None,
                    is_error: Some(false),
                }),
            )
        })
    }
}

pub struct AllowToolHook;

impl ToolCallHook for AllowToolHook {
    fn id(&self) -> Option<&str> {
        Some("allow-tool-hook")
    }

    fn before_tool_call<'a>(
        &'a self,
        _context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async {
            Ok(Some(BeforeToolCallResult::decision(
                ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("allowed by test hook".into()),
                    approver: Some("test".into()),
                    metadata: json!({ "source": "allow-test" }),
                },
            )))
        })
    }
}

pub struct BarrierAllowToolHook {
    pub barrier: Arc<Barrier>,
}

impl ToolCallHook for BarrierAllowToolHook {
    fn id(&self) -> Option<&str> {
        Some("barrier-allow-tool-hook")
    }

    fn before_tool_call<'a>(
        &'a self,
        _context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        let barrier = Arc::clone(&self.barrier);
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            barrier.wait().await;
            cancellation.throw_if_cancelled()?;
            Ok(Some(BeforeToolCallResult::decision(
                ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("allowed by barrier test hook".into()),
                    approver: Some("test".into()),
                    metadata: json!({ "source": "barrier-allow-test" }),
                },
            )))
        })
    }
}

pub struct ApprovalToolHook {
    pub expires_at_ms: Option<u64>,
}

impl ToolCallHook for ApprovalToolHook {
    fn id(&self) -> Option<&str> {
        Some("approval-tool-hook")
    }

    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            Ok(
                (context.tool_call.name == "fast").then_some(BeforeToolCallResult::approval(
                    ToolApprovalRequestSpec {
                        prompt: Some("Approve fast tool?".into()),
                        reason: Some("test approval gate".into()),
                        expires_at_ms: self.expires_at_ms,
                        metadata: json!({ "source": "approval-test" }),
                    },
                )),
            )
        })
    }
}

pub struct MediaRewriteHook;

impl ToolCallHook for MediaRewriteHook {
    fn after_tool_call<'a>(
        &'a self,
        _context: AfterToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterToolCallResult>> {
        Box::pin(async {
            Ok(Some(AfterToolCallResult {
                content: Some(vec![ContentBlock::Media {
                    media: MediaBlock::provider(MediaKind::File, "hook-provider", "file-1"),
                }]),
                details: None,
                is_error: Some(false),
            }))
        })
    }
}
