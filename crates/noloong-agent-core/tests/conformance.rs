use noloong_agent_core::{
    Agent, AgentCoreError, AgentEventKind, AgentInput, AgentMessage, AgentRuntime, BoxFuture,
    CancellationToken, ContentBlock, EventStore, InMemoryEventStore, MessageRole, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, QueueMode, Result, RunStatus,
    StdioExtensionConfig, StopReason, ToolCall, ToolExecutionMode, ToolOutput, ToolProvider,
    ToolRequest, ToolSpec, reduce_events,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, mpsc},
    time::{sleep, timeout},
};

pub mod support;

use support::fixture_path;

#[tokio::test]
async fn runtime_success_replay_matches_report_state() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TextModel))
        .build()?;

    let report = runtime.run("hello").await?;

    assert_eq!(reduce_events(&report.events)?, report.state);
    assert!(matches!(report.state.status, RunStatus::Completed));
    Ok(())
}

#[tokio::test]
async fn runtime_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(FailingModel))
        .build()?;

    let error = runtime.run("fail").await.unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(error.to_string().contains("model stream failed"));
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, AgentEventKind::PhaseFailed { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, AgentEventKind::RunFailed { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn runtime_abort_records_aborted_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = Arc::new(
        AgentRuntime::builder()
            .with_event_store(event_store.clone())
            .with_model_provider(Arc::new(BlockingModel))
            .build()?,
    );
    let cancellation = CancellationToken::new();
    let run_runtime = Arc::clone(&runtime);
    let run_cancellation = cancellation.clone();

    let handle = tokio::spawn(async move {
        run_runtime
            .run_with_event_sink(
                AgentInput::from("block"),
                Arc::new(|_event| Box::pin(async { Ok(()) })),
                run_cancellation,
            )
            .await
    });
    sleep(Duration::from_millis(50)).await;
    cancellation.cancel();

    let error = handle.await?.unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(matches!(error, AgentCoreError::Aborted));
    assert!(matches!(state.status, RunStatus::Aborted));
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, AgentEventKind::RunAborted))
    );
    Ok(())
}

#[tokio::test]
async fn event_store_contains_event_before_sink_notification() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(TextModel))
        .build()?;

    runtime
        .run_with_events("hello", move |event| {
            let event_store = event_store.clone();
            async move {
                let stored_events = event_store.load(&event.run_id).await?;
                assert!(
                    stored_events
                        .iter()
                        .any(|stored| stored.sequence == event.sequence)
                );
                Ok(())
            }
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn event_sink_failure_does_not_notify_run_failed_to_failing_sink() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(TextModel))
        .build()?;
    let notified = Arc::new(Mutex::new(Vec::new()));
    let notified_events = Arc::clone(&notified);

    let error = runtime
        .run_with_events("hello", move |event| {
            let notified_events = Arc::clone(&notified_events);
            async move {
                notified_events.lock().await.push(event.kind.clone());
                if matches!(event.kind, AgentEventKind::TurnStarted) {
                    Err(AgentCoreError::EventSink("sink failed".into()))
                } else {
                    Ok(())
                }
            }
        })
        .await
        .unwrap_err();

    let stored_events = event_store.load("run-1").await?;
    assert!(matches!(error, AgentCoreError::EventSink(_)));
    assert!(
        stored_events
            .iter()
            .any(|event| matches!(event.kind, AgentEventKind::RunFailed { .. }))
    );
    assert!(
        !notified
            .lock()
            .await
            .iter()
            .any(|kind| matches!(kind, AgentEventKind::RunFailed { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn event_model_stream_events_are_not_duplicated_when_provider_pushes_and_returns()
-> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TextModel))
        .build()?;

    let report = runtime.run("hello").await?;
    let text_delta_count = report
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ModelStreamEvent {
                    event: ModelStreamEvent::TextDelta { .. },
                    ..
                }
            )
        })
        .count();

    assert_eq!(text_delta_count, 1);
    Ok(())
}

#[tokio::test]
async fn queue_one_at_a_time_drains_multiple_follow_ups_across_turns() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingTextModel::default()))
        .build()?;
    agent.set_follow_up_mode(QueueMode::OneAtATime);
    agent.follow_up(AgentMessage::user("follow-up-1", "second"));
    agent.follow_up(AgentMessage::user("follow-up-2", "third"));

    agent.prompt("first").await?;
    let state = agent.state().await;

    assert_eq!(state.completed_turns, 3);
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.id == "follow-up-1")
    );
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.id == "follow-up-2")
    );
    Ok(())
}

#[tokio::test]
async fn steering_waits_until_tool_batch_completes() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(ToolBatchModel::default()))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(50),
            None,
        )))
        .with_tool(Arc::new(DelayedTool::new(
            "fast",
            Duration::from_millis(0),
            None,
        )))
        .build()?;
    let steering_agent = agent.clone();
    let steered = Arc::new(AtomicBool::new(false));
    let steered_flag = Arc::clone(&steered);
    agent.subscribe(move |event| {
        let steering_agent = steering_agent.clone();
        let steered_flag = Arc::clone(&steered_flag);
        async move {
            if matches!(event.kind, AgentEventKind::ToolExecutionStarted { .. })
                && !steered_flag.swap(true, Ordering::SeqCst)
            {
                steering_agent.steer(AgentMessage::user("steer-after-batch", "steer"));
            }
            Ok(())
        }
    });

    agent.prompt("tools").await?;
    let state = agent.state().await;

    assert!(state.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if text == "batch complete"))
    }));
    Ok(())
}

#[tokio::test]
async fn tool_policy_modes_commit_source_order() -> Result<()> {
    let parallel_order = committed_tool_names(ToolExecutionMode::Parallel, None).await?;
    let sequential_order = committed_tool_names(ToolExecutionMode::Sequential, None).await?;
    let per_tool_order = committed_tool_names(
        ToolExecutionMode::Parallel,
        Some(ToolExecutionMode::Sequential),
    )
    .await?;

    assert_eq!(parallel_order, vec!["slow".to_string(), "fast".to_string()]);
    assert_eq!(
        sequential_order,
        vec!["slow".to_string(), "fast".to_string()]
    );
    assert_eq!(per_tool_order, vec!["slow".to_string(), "fast".to_string()]);
    Ok(())
}

#[tokio::test]
async fn jsonrpc_stream_event_arrives_before_response() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--delayed-stream".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;
    let (sender, mut receiver) = mpsc::channel(1);

    let run = tokio::spawn(async move {
        runtime
            .run_with_events("hello", move |event| {
                let sender = sender.clone();
                async move {
                    if let AgentEventKind::ModelStreamEvent {
                        event: ModelStreamEvent::TextDelta { text },
                        ..
                    } = event.kind
                    {
                        let _ = sender.send(text).await;
                    }
                    Ok(())
                }
            })
            .await
    });

    let text = timeout(Duration::from_millis(75), receiver.recv())
        .await
        .expect("stream event should arrive before delayed response")
        .expect("stream event should be sent");
    run.await??;

    assert_eq!(text, "delayed chunk");
    Ok(())
}

#[tokio::test]
async fn jsonrpc_finished_settles_without_response() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--stream-no-response".into(),
                ])
                .request_timeout(Duration::from_secs(5))
                .stream_timeout(Duration::from_millis(500)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = timeout(Duration::from_millis(200), runtime.run("hello"))
        .await
        .expect("terminal stream event should settle without JSON-RPC response")?;

    assert!(report.state.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if text == "terminal chunk"))
    }));
    Ok(())
}

#[tokio::test]
async fn jsonrpc_request_timeout_is_structured() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--request-timeout-on-model".into(),
                ])
                .request_timeout(Duration::from_millis(500))
                .stream_timeout(Duration::from_secs(5)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let error = timeout(Duration::from_secs(2), runtime.run("hello"))
        .await
        .expect("request timeout should fire before stream timeout")
        .unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(
        error
            .to_string()
            .contains("request timed out: model/stream")
    );
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

async fn committed_tool_names(
    mode: ToolExecutionMode,
    slow_tool_mode: Option<ToolExecutionMode>,
) -> Result<Vec<String>> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(25),
            slow_tool_mode,
        )))
        .with_tool(Arc::new(DelayedTool::new(
            "fast",
            Duration::from_millis(0),
            None,
        )))
        .with_tool_execution_mode(mode)
        .max_turns(1)
        .build()?;
    let report = runtime.run("tools").await?;

    Ok(report
        .state
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult { tool_name, .. }) => Some(tool_name.clone()),
            _ => None,
        })
        .collect())
}

struct TextModel;

impl ModelProvider for TextModel {
    fn id(&self) -> &str {
        "text"
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
                    stream_id: "text-stream".into(),
                },
                ModelStreamEvent::TextDelta { text: "ok".into() },
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

struct FailingModel;

impl ModelProvider for FailingModel {
    fn id(&self) -> &str {
        "failing"
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

struct BlockingModel;

impl ModelProvider for BlockingModel {
    fn id(&self) -> &str {
        "blocking"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        _stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.cancelled().await;
            Err(AgentCoreError::Aborted)
        })
    }
}

#[derive(Default)]
struct CountingTextModel {
    calls: AtomicU64,
}

impl ModelProvider for CountingTextModel {
    fn id(&self) -> &str {
        "counting-text"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: format!("counting-{call}"),
                },
                ModelStreamEvent::TextDelta {
                    text: format!("turn {call}"),
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

#[derive(Default)]
struct ToolBatchModel {
    calls: AtomicU64,
}

impl ModelProvider for ToolBatchModel {
    fn id(&self) -> &str {
        "tool-batch"
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
                two_tool_events()
            } else {
                let tool_result_count = request
                    .messages
                    .iter()
                    .filter(|message| matches!(message.role, MessageRole::ToolResult))
                    .count();
                assert_eq!(tool_result_count, 2);
                assert!(
                    request
                        .messages
                        .iter()
                        .any(|message| message.id == "steer-after-batch")
                );
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "tool-batch-finished".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "batch complete".into(),
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

struct TwoToolModel;

impl ModelProvider for TwoToolModel {
    fn id(&self) -> &str {
        "two-tool"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let events = two_tool_events();
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

fn two_tool_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::Started {
            stream_id: "two-tool-stream".into(),
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

struct DelayedTool {
    name: &'static str,
    delay: Duration,
    execution_mode: Option<ToolExecutionMode>,
}

impl DelayedTool {
    fn new(name: &'static str, delay: Duration, execution_mode: Option<ToolExecutionMode>) -> Self {
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
            description: "Conformance delayed tool".into(),
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
