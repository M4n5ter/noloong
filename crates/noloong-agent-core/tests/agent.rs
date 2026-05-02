use noloong_agent_core::{
    Agent, AgentCoreError, AgentEventKind, AgentMessage, BoxFuture, CancellationToken,
    ContentBlock, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink, QueueMode,
    Result, RunStatus, StopReason, ToolCall, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn agent_prompt_preserves_transcript_across_runs() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .build()?;

    agent.prompt("first").await?;
    agent.prompt("second").await?;

    let state = agent.state().await;
    assert_eq!(state.messages.len(), 4);
    assert!(matches!(state.status, RunStatus::Completed));
    Ok(())
}

#[tokio::test]
async fn agent_continue_run_validates_last_message_role() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .with_initial_messages(vec![AgentMessage::user("user-1", "continue")])
        .build()?;

    agent.continue_run().await?;
    let error = agent.continue_run().await.unwrap_err();

    assert!(error.to_string().contains("cannot continue from assistant"));
    Ok(())
}

#[tokio::test]
async fn agent_abort_cancels_active_run() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(BlockingModel))
        .build()?;
    let running_agent = agent.clone();
    let handle = tokio::spawn(async move { running_agent.prompt("block").await });

    sleep(Duration::from_millis(50)).await;
    agent.abort().await;
    let error = handle.await.expect("prompt task joins").unwrap_err();

    assert!(matches!(error, AgentCoreError::Aborted));
    assert!(matches!(agent.state().await.status, RunStatus::Aborted));
    Ok(())
}

#[tokio::test]
async fn wait_for_idle_waits_for_subscriber_barrier() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .build()?;
    let subscriber_finished = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&subscriber_finished);
    agent.subscribe(move |event| {
        let flag = Arc::clone(&flag);
        async move {
            if matches!(event.kind, AgentEventKind::RunCompleted) {
                sleep(Duration::from_millis(50)).await;
                flag.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
    });

    let running_agent = agent.clone();
    let handle = tokio::spawn(async move { running_agent.prompt("hello").await });
    agent.wait_for_idle().await;
    handle.await.expect("prompt task joins")?;

    assert!(subscriber_finished.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn follow_up_runs_after_agent_would_stop() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .build()?;
    agent.follow_up(AgentMessage::user("follow-up", "second turn"));

    agent.prompt("first turn").await?;

    let state = agent.state().await;
    assert_eq!(state.messages.len(), 4);
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.id == "follow-up")
    );
    Ok(())
}

#[tokio::test]
async fn queue_mode_all_drains_multiple_follow_ups_into_one_turn() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .build()?;
    agent.set_follow_up_mode(QueueMode::All);
    agent.follow_up(AgentMessage::user("follow-up-1", "second turn"));
    agent.follow_up(AgentMessage::user("follow-up-2", "same turn"));

    agent.prompt("first turn").await?;

    let state = agent.state().await;
    assert_eq!(state.completed_turns, 2);
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
async fn steering_is_injected_after_tool_batch() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(ToolCallingModel::default()))
        .with_tool(Arc::new(EchoTool))
        .build()?;
    let steering_agent = agent.clone();
    agent.subscribe(move |event| {
        let steering_agent = steering_agent.clone();
        async move {
            if matches!(event.kind, AgentEventKind::ToolExecutionStarted { .. }) {
                steering_agent.steer(AgentMessage::user("steer-1", "steered"));
            }
            Ok(())
        }
    });

    agent.prompt("use tool").await?;

    let state = agent.state().await;
    assert!(state.messages.iter().any(|message| message.id == "steer-1"));
    assert!(state.messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Text { text } if text.contains("saw steering")),
        )
    }));
    Ok(())
}

#[derive(Default)]
struct CountingModel {
    calls: AtomicU64,
}

impl ModelProvider for CountingModel {
    fn id(&self) -> &str {
        "counting"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: format!("counting-{call}"),
                },
                ModelStreamEvent::TextDelta {
                    text: format!("response {call}"),
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
struct ToolCallingModel {
    calls: AtomicU64,
}

impl ModelProvider for ToolCallingModel {
    fn id(&self) -> &str {
        "tool-calling"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "tool-calling-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "tool-call-1".into(),
                            name: "echo".into(),
                            arguments: json!({ "text": "tool result" }),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                assert!(
                    request
                        .messages
                        .iter()
                        .any(|message| message.id == "steer-1")
                );
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "tool-calling-2".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "saw steering".into(),
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

struct EchoTool;

impl ToolProvider for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "Echo test tool".into(),
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
