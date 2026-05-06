use noloong_agent_core::{
    Agent, AgentCoreError, AgentEventKind, AgentMessage, AgentState, BeforeToolCallContext,
    BeforeToolCallResult, BoxFuture, CancellationToken, ContentBlock, ModelProvider, ModelRequest,
    ModelStreamEvent, ModelStreamSink, QueueMode, QueuedAgentMessage, QueuedMessageIntent, Result,
    RunStatus, StopReason, ToolApprovalRequestSpec, ToolApprovalResolution, ToolCall, ToolCallHook,
    ToolOutput, ToolPermissionDecision, ToolPermissionOutcome, ToolProvider, ToolRequest, ToolSpec,
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
async fn agent_builder_restores_initial_state() -> Result<()> {
    let initial_state = AgentState {
        status: RunStatus::Completed,
        messages: vec![AgentMessage::user("restored-user", "restored")],
        completed_turns: 7,
        ..AgentState::default()
    };
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .with_initial_state(initial_state.clone())
        .build()?;

    assert_eq!(agent.state().await, initial_state);
    Ok(())
}

#[tokio::test]
async fn queued_modes_are_readable() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CountingModel::default()))
        .build()?;

    assert_eq!(agent.steering_queue_mode(), QueueMode::OneAtATime);
    assert_eq!(agent.follow_up_queue_mode(), QueueMode::OneAtATime);

    agent.set_steering_mode(QueueMode::All);
    agent.set_follow_up_mode(QueueMode::All);

    assert_eq!(agent.steering_queue_mode(), QueueMode::All);
    assert_eq!(agent.follow_up_queue_mode(), QueueMode::All);
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
async fn agent_approval_api_lists_pending_and_resumes_paused_run() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(ApprovalModel::default()))
        .with_tool(Arc::new(EchoTool))
        .with_tool_hook(Arc::new(ApprovalHook))
        .build()?;

    agent.prompt("approval").await?;

    let pending = agent.pending_tool_approvals().await;
    assert_eq!(pending.len(), 1);
    assert!(matches!(agent.state().await.status, RunStatus::Paused));
    let approval_id = pending.keys().next().expect("approval id exists").clone();
    agent
        .resume_tool_approval(ToolApprovalResolution {
            approval_id,
            decision: ToolPermissionDecision {
                outcome: ToolPermissionOutcome::Allow,
                reason: Some("approved by agent test".into()),
                approver: Some("agent-test".into()),
                metadata: json!({}),
            },
        })
        .await?;

    let state = agent.state().await;
    assert!(matches!(state.status, RunStatus::Completed));
    assert!(state.pending_tool_approvals.is_empty());
    assert!(state.messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Text { text } if text == "approval complete"),
        )
    }));
    Ok(())
}

#[tokio::test]
async fn agent_abort_clears_paused_approval_run() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(ApprovalModel::default()))
        .with_tool(Arc::new(EchoTool))
        .with_tool_hook(Arc::new(ApprovalHook))
        .build()?;

    agent.prompt("approval").await?;
    agent.abort().await;

    let state = agent.state().await;
    assert!(matches!(state.status, RunStatus::Aborted));
    assert!(state.pending_tool_approvals.is_empty());
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
async fn queued_steering_is_injected_before_first_model_request() -> Result<()> {
    let model = Arc::new(CapturingModel::default());
    let agent = Agent::builder()
        .with_model_provider(model.clone())
        .build()?;
    agent.steer(AgentMessage::user(
        "queued-steer",
        "background job completed",
    ));

    agent.prompt("next user prompt").await?;

    let requests = model
        .requests
        .lock()
        .expect("captured requests lock poisoned");
    let messages = &requests
        .first()
        .expect("first model request exists")
        .messages;
    let steering_index = message_index(messages, "queued-steer");
    let prompt_index = message_index(messages, "user-run-1-1");

    assert!(steering_index < prompt_index);
    assert_eq!(messages[steering_index].role.as_str(), "user");
    assert_eq!(messages[prompt_index].role.as_str(), "user");
    Ok(())
}

#[tokio::test]
async fn queued_messages_are_editable() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(CapturingModel::default()))
        .build()?;

    agent.steer(AgentMessage::user("drop-me", "drop"));
    agent.steer_user_input(AgentMessage::user("keep-me", "before edit"));
    agent.edit_steering_queue(|messages| {
        messages.retain(|message| message.message.id != "drop-me");
        messages.push(QueuedAgentMessage::observation(AgentMessage::user(
            "inserted",
            "inserted observation",
        )));
        messages[0].message.content = vec![ContentBlock::Text {
            text: "after edit".into(),
        }];
    });

    let messages = agent.queued_steering_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].message.id, "keep-me");
    assert_eq!(messages[0].intent, QueuedMessageIntent::UserInput);
    assert!(matches!(
        messages[0].message.content.first(),
        Some(ContentBlock::Text { text }) if text == "after edit"
    ));
    assert_eq!(messages[1].message.id, "inserted");
    assert_eq!(messages[1].intent, QueuedMessageIntent::Observation);
    Ok(())
}

#[tokio::test]
async fn stopped_turn_routes_user_input_steering_through_follow_up_mode() -> Result<()> {
    let model = Arc::new(CapturingModel::default());
    let agent = Agent::builder()
        .with_model_provider(model.clone())
        .build()?;
    agent.set_steering_mode(QueueMode::All);
    agent.set_follow_up_mode(QueueMode::OneAtATime);
    let injected = Arc::new(AtomicBool::new(false));
    let steering_agent = agent.clone();
    agent.subscribe(move |event| {
        let steering_agent = steering_agent.clone();
        let injected = Arc::clone(&injected);
        async move {
            if matches!(event.kind, AgentEventKind::TurnCompleted { .. })
                && !injected.swap(true, Ordering::SeqCst)
            {
                steering_agent.steer_user_input(AgentMessage::user("typed-1", "first typed"));
                steering_agent.steer_user_input(AgentMessage::user("typed-2", "second typed"));
            }
            Ok(())
        }
    });

    agent.prompt("initial").await?;

    let requests = model
        .requests
        .lock()
        .expect("captured requests lock poisoned");
    assert_eq!(requests.len(), 3);
    let second_messages = &requests[1].messages;
    assert!(
        second_messages
            .iter()
            .any(|message| message.id == "typed-1")
    );
    assert!(
        !second_messages
            .iter()
            .any(|message| message.id == "typed-2")
    );
    let third_messages = &requests[2].messages;
    assert!(third_messages.iter().any(|message| message.id == "typed-2"));
    drop(requests);

    let follow_up = agent.queued_follow_up_messages();
    assert!(follow_up.is_empty());
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
struct CapturingModel {
    requests: std::sync::Mutex<Vec<ModelRequest>>,
}

impl ModelProvider for CapturingModel {
    fn id(&self) -> &str {
        "capturing"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            self.requests
                .lock()
                .expect("captured requests lock poisoned")
                .push(request);
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "capturing-1".into(),
                },
                ModelStreamEvent::TextDelta {
                    text: "captured".into(),
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

fn message_index(messages: &[AgentMessage], id: &str) -> usize {
    messages
        .iter()
        .position(|message| message.id == id)
        .expect("message exists")
}

#[derive(Default)]
struct ApprovalModel {
    calls: AtomicU64,
}

impl ModelProvider for ApprovalModel {
    fn id(&self) -> &str {
        "approval"
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
                        stream_id: "approval-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "approval-call-1".into(),
                            name: "echo".into(),
                            arguments: json!({ "text": "approved tool result" }),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "approval-2".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "approval complete".into(),
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

struct ApprovalHook;

impl ToolCallHook for ApprovalHook {
    fn id(&self) -> Option<&str> {
        Some("approval-hook")
    }

    fn before_tool_call<'a>(
        &'a self,
        _context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async {
            Ok(Some(BeforeToolCallResult::approval(
                ToolApprovalRequestSpec {
                    prompt: Some("Approve echo?".into()),
                    reason: Some("agent test approval".into()),
                    expires_at_ms: None,
                    metadata: json!({}),
                },
            )))
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
            permissions: Vec::new(),
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
