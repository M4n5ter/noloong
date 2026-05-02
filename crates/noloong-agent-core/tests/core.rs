use noloong_agent_core::{
    AfterToolCallContext, AfterToolCallResult, AgentCoreError, AgentEffect, AgentEventKind,
    AgentMessage, AgentRuntime, BeforeToolCallContext, BeforeToolCallResult, BoxFuture,
    CancellationToken, ContentBlock, ContextPatch, ContextProvider, ContextRequest, EventStore,
    InMemoryEventStore, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink,
    PHASE_CONTEXT_PREPARE, PhaseContext, PhaseNode, PhaseOutput, Result, RunStatus, StopReason,
    ToolCall, ToolCallHook, ToolExecutionMode, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
    ToolUpdate, reduce_events,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn event_log_replays_to_report_state() -> Result<()> {
    let runtime = native_runtime().build()?;

    let report = runtime.run("hello").await?;
    let replayed = reduce_events(&report.events)?;

    assert_eq!(report.state, replayed);
    assert_eq!(report.state.context.get("native"), Some(&json!("context")));
    assert_eq!(report.state.completed_turns, 2);
    assert_eq!(report.state.messages.len(), 4);
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { tool_call_id, .. }
                if tool_call_id == "call-1"
        )
    }));
    Ok(())
}

#[tokio::test]
async fn phase_graph_allows_inserting_effectful_phase() -> Result<()> {
    let runtime = native_runtime()
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(InsertedPhase))
        .build()?;

    let report = runtime.run("hello").await?;

    assert_eq!(report.state.context.get("inserted"), Some(&json!(true)));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseStarted { phase } if phase == "test.inserted"
        )
    }));
    Ok(())
}

#[tokio::test]
async fn invalid_effect_is_rejected_and_fails_run() -> Result<()> {
    let runtime = native_runtime()
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(InvalidEffectPhase))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();
    assert!(matches!(error, AgentCoreError::InvalidEffect(_)));
    Ok(())
}

#[tokio::test]
async fn run_with_events_emits_realtime_events_in_order() -> Result<()> {
    let runtime = native_runtime().build()?;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&events);

    runtime
        .run_with_events("hello", move |event| {
            let received = Arc::clone(&received);
            async move {
                received.lock().await.push(event.kind);
                Ok(())
            }
        })
        .await?;

    let events = events.lock().await;
    assert!(matches!(events.first(), Some(AgentEventKind::RunStarted)));
    assert!(matches!(events.get(1), Some(AgentEventKind::TurnStarted)));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEventKind::ModelStreamEvent { .. }))
    );
    assert!(matches!(events.last(), Some(AgentEventKind::RunCompleted)));
    Ok(())
}

#[tokio::test]
async fn event_sink_failure_records_run_failed() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = native_runtime()
        .with_event_store(event_store.clone())
        .build()?;

    let error = runtime
        .run_with_events("hello", |event| async move {
            if matches!(event.kind, AgentEventKind::TurnStarted) {
                Err(AgentCoreError::EventSink("boom".into()))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap_err();

    assert!(matches!(error, AgentCoreError::EventSink(_)));
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn model_stream_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(FailingModel))
        .build()?;

    let error = runtime.run("fail").await.unwrap_err();

    assert!(error.to_string().contains("model stream failed"));
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::Failed { .. },
                ..
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn tool_failure_becomes_auditable_tool_result() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(FailingTool("slow")))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;

    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    is_error: true,
                    ..
                } if tool_name == "slow"
            )
        })
    }));
    assert_eq!(reduce_events(&report.events)?, report.state);
    Ok(())
}

#[tokio::test]
async fn context_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(NativeModel {
            calls: AtomicU64::new(0),
        }))
        .with_context_provider(Arc::new(FailingContext))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("context failed"));
    let state = reduce_events(&event_store.load("run-1").await?)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn phase_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = native_runtime()
        .with_event_store(event_store.clone())
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(FailingPhase))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("phase failed"));
    let state = reduce_events(&event_store.load("run-1").await?)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn parallel_tools_emit_completion_order_but_commit_source_order() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(50),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    let report = runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted {
                    tool_call_id,
                    output: _,
                } = event.kind
                {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["fast-call", "slow-call"]
    );
    let committed_tool_names = report
        .state
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult { tool_name, .. }) => Some(tool_name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(committed_tool_names, ["slow", "fast"]);
    Ok(())
}

#[tokio::test]
async fn sequential_tools_emit_source_order() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(20),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .with_tool_execution_mode(ToolExecutionMode::Sequential)
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted { tool_call_id, .. } = event.kind {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["slow-call", "fast-call"]
    );
    Ok(())
}

#[tokio::test]
async fn per_tool_execution_mode_can_force_sequential() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new_with_mode(
            "slow",
            Duration::from_millis(20),
            Some(ToolExecutionMode::Sequential),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted { tool_call_id, .. } = event.kind {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["slow-call", "fast-call"]
    );
    Ok(())
}

#[tokio::test]
async fn tool_hooks_can_block_and_rewrite_results() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new("slow", Duration::from_millis(0))))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .with_tool_hook(Arc::new(TestToolHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;
    let tool_results = report
        .state
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            }) => Some((tool_name.clone(), content.clone(), *is_error)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_results.len(), 2);
    assert!(
        tool_results
            .iter()
            .any(|(name, _, is_error)| { name == "slow" && *is_error })
    );
    assert!(tool_results.iter().any(|(name, content, is_error)| {
        name == "fast"
            && !*is_error
            && matches!(content.first(), Some(ContentBlock::Text { text }) if text == "rewritten")
    }));
    Ok(())
}

fn native_runtime() -> noloong_agent_core::AgentRuntimeBuilder {
    AgentRuntime::builder()
        .with_model_provider(Arc::new(NativeModel {
            calls: AtomicU64::new(0),
        }))
        .with_tool(Arc::new(NativeTool))
        .with_context_provider(Arc::new(NativeContext))
        .max_turns(4)
}

struct NativeModel {
    calls: AtomicU64,
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

struct NativeTool;

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

struct NativeContext;

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

struct InsertedPhase;

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

struct InvalidEffectPhase;

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

struct FailingModel;

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

struct FailingContext;

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

struct FailingPhase;

impl PhaseNode for FailingPhase {
    fn id(&self) -> &str {
        "test.failing-phase"
    }

    fn run<'a>(&'a self, _context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async { Err(AgentCoreError::Phase("phase failed".into())) })
    }
}

struct TwoToolModel;

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
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "two-tools".into(),
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
            ];
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

struct DelayedTool {
    name: &'static str,
    delay: Duration,
    execution_mode: Option<ToolExecutionMode>,
}

struct FailingTool(&'static str);

impl ToolProvider for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.0.into(),
            description: "Failing test tool".into(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
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
    fn new(name: &'static str, delay: Duration) -> Self {
        Self {
            name,
            delay,
            execution_mode: None,
        }
    }

    fn new_with_mode(
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

struct TestToolHook;

impl ToolCallHook for TestToolHook {
    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            Ok(
                (context.tool_call.name == "slow").then_some(BeforeToolCallResult {
                    block: true,
                    reason: Some("blocked by test hook".into()),
                }),
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
