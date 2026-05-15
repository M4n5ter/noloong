use noloong_agent::{
    AgentSession, Catalog, Locale, SubagentController, SubagentFinalOutput, SubagentListTool,
    SubagentResult, SubagentResultTool, SubagentSpawnRequest, SubagentSpawnTool, SubagentSummary,
    SubagentWaitOutcome, SubagentWaitTool,
};
use noloong_agent_core::{
    AgentEventKind, AgentMessage, AgentState, BoxFuture, CancellationToken, ContentBlock,
    ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink, StopReason, ToolCall,
    ToolOutput, ToolProvider, ToolRequest,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn subagent_spawn_tool_trims_prompt_and_role() {
    let controller = Arc::new(FakeSubagentController::default());
    let tool = SubagentSpawnTool::new(controller.clone(), Catalog::new(Locale::En));

    let output = tool
        .execute_tool(
            request(json!({
                "role": " reviewer ",
                "prompt": " check this ",
                "metadata": {"topic": "tests"}
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(output.details["sessionId"], "child-1");
    let recorded = controller.spawn_requests.lock().unwrap();
    assert_eq!(recorded[0].role.as_deref(), Some("reviewer"));
    assert_eq!(recorded[0].prompt, "check this");
    assert_eq!(recorded[0].metadata["topic"], "tests");
}

#[tokio::test]
async fn subagent_wait_tool_validates_timeout_and_returns_results() {
    let controller = Arc::new(FakeSubagentController::default());
    let tool = SubagentWaitTool::new(controller.clone(), Catalog::new(Locale::En));

    let output = tool
        .execute_tool(
            request(json!({"sessionIds": ["child-1"], "timeoutMs": 5})),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(output.details["timedOut"], false);
    assert_eq!(output.details["results"][0]["sessionId"], "child-1");
    assert_eq!(controller.wait_requests.lock().unwrap()[0].1, 5);

    let error = tool
        .execute_tool(
            request(json!({"sessionIds": ["child-1"], "timeoutMs": 0})),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("timeoutMs must be between"));
}

#[tokio::test]
async fn subagent_result_and_list_tools_return_json() {
    let controller = Arc::new(FakeSubagentController::default());
    let result_tool = SubagentResultTool::new(controller.clone(), Catalog::new(Locale::En));
    let list_tool = SubagentListTool::new(controller, Catalog::new(Locale::En));

    let result = result_tool
        .execute_tool(
            request(json!({"sessionId": "child-1"})),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.details["finalOutput"]["finalText"], "final text");

    let list = list_tool
        .execute_tool(request(json!({})), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(list.details["subagents"][0]["status"], "completed");
}

#[test]
fn final_output_extracts_last_assistant_text() {
    let state = AgentState {
        messages: vec![
            AgentMessage::assistant("a1", vec![ContentBlock::Text { text: "old".into() }]),
            AgentMessage::user("u1", "next"),
            AgentMessage::assistant(
                "a2",
                vec![
                    ContentBlock::Text {
                        text: "first".into(),
                    },
                    ContentBlock::Json {
                        value: json!({"kept": true}),
                    },
                    ContentBlock::Text {
                        text: "second".into(),
                    },
                ],
            ),
        ],
        ..AgentState::default()
    };

    let output = noloong_agent::tools::final_assistant_output(&state).unwrap();
    assert_eq!(output.message.id, "a2");
    assert_eq!(output.final_text, "first\nsecond");
}

#[tokio::test]
async fn subagent_result_reuses_tool_output_overflow_hook() {
    let temp_dir = unique_temp_dir("subagent-overflow");
    let session = AgentSession::builder()
        .with_subagent_controller(Arc::new(LargeOutputSubagentController))
        .with_max_inline_tool_output_bytes(128)
        .with_tool_output_temp_dir(temp_dir.clone())
        .build();
    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(SubagentResultCallModel::default()))
        .build()
        .unwrap();

    let report = runtime.run("read subagent result").await.unwrap();
    let output = report
        .events
        .iter()
        .find_map(|event| match &event.kind {
            AgentEventKind::ToolExecutionCompleted {
                tool_call_id,
                output,
            } if tool_call_id == "subagent-result-large" => Some(output),
            _ => None,
        })
        .expect("subagent tool completion should exist");
    let path = output.details["path"]
        .as_str()
        .expect("overflow path should exist");
    let stored = read_stored_output(path).await;

    assert_eq!(output.details["overflow"].as_bool(), Some(true));
    assert!(Path::new(path).starts_with(&temp_dir));
    assert_eq!(
        stored.details["finalOutput"]["finalText"]
            .as_str()
            .expect("stored final text exists"),
        large_final_text()
    );
}

#[derive(Default)]
struct FakeSubagentController {
    spawn_requests: Mutex<Vec<SubagentSpawnRequest>>,
    wait_requests: Mutex<Vec<(Vec<String>, u64)>>,
}

impl SubagentController for FakeSubagentController {
    fn spawn_subagent<'a>(
        &'a self,
        request: SubagentSpawnRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentSummary> {
        Box::pin(async move {
            self.spawn_requests.lock().unwrap().push(request);
            Ok(summary("child-1"))
        })
    }

    fn wait_subagents<'a>(
        &'a self,
        session_ids: Vec<String>,
        timeout_ms: u64,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentWaitOutcome> {
        Box::pin(async move {
            self.wait_requests
                .lock()
                .unwrap()
                .push((session_ids.clone(), timeout_ms));
            Ok(SubagentWaitOutcome {
                timed_out: false,
                results: session_ids
                    .into_iter()
                    .map(|session_id| result(&session_id))
                    .collect(),
            })
        })
    }

    fn subagent_result<'a>(
        &'a self,
        session_id: String,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentResult> {
        Box::pin(async move { Ok(result(&session_id)) })
    }

    fn list_subagents<'a>(
        &'a self,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<SubagentSummary>> {
        Box::pin(async move { Ok(vec![summary("child-1")]) })
    }
}

fn result(session_id: &str) -> SubagentResult {
    SubagentResult {
        summary: summary(session_id),
        settled: true,
        final_output: Some(SubagentFinalOutput {
            message: AgentMessage::assistant(
                "assistant-final",
                vec![ContentBlock::Text {
                    text: "final text".into(),
                }],
            ),
            final_text: "final text".into(),
        }),
    }
}

fn summary(session_id: &str) -> SubagentSummary {
    SubagentSummary {
        session_id: session_id.into(),
        role: Some("reviewer".into()),
        status: "completed".into(),
    }
}

fn request(arguments: serde_json::Value) -> ToolRequest {
    ToolRequest {
        run_id: "run-test".into(),
        turn_id: 1,
        tool_call_id: "tool-call-test".into(),
        tool_name: "agent.subagent.test".into(),
        arguments,
        state: AgentState::default(),
    }
}

#[derive(Default)]
struct SubagentResultCallModel {
    calls: AtomicU64,
}

impl ModelProvider for SubagentResultCallModel {
    fn id(&self) -> &str {
        "subagent-result-call"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let events = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "subagent-result-call-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "subagent-result-large".into(),
                            name: "agent.subagent.result".into(),
                            arguments: json!({"sessionId": "child-large"}),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "subagent-result-call-2".into(),
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

struct LargeOutputSubagentController;

impl SubagentController for LargeOutputSubagentController {
    fn spawn_subagent<'a>(
        &'a self,
        _request: SubagentSpawnRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentSummary> {
        Box::pin(async { Ok(summary("child-large")) })
    }

    fn wait_subagents<'a>(
        &'a self,
        session_ids: Vec<String>,
        _timeout_ms: u64,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentWaitOutcome> {
        Box::pin(async move {
            Ok(SubagentWaitOutcome {
                timed_out: false,
                results: session_ids
                    .into_iter()
                    .map(|session_id| large_result(&session_id))
                    .collect(),
            })
        })
    }

    fn subagent_result<'a>(
        &'a self,
        session_id: String,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentResult> {
        Box::pin(async move { Ok(large_result(&session_id)) })
    }

    fn list_subagents<'a>(
        &'a self,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<SubagentSummary>> {
        Box::pin(async { Ok(vec![summary("child-large")]) })
    }
}

fn large_result(session_id: &str) -> SubagentResult {
    let final_text = large_final_text();
    SubagentResult {
        summary: summary(session_id),
        settled: true,
        final_output: Some(SubagentFinalOutput {
            message: AgentMessage::assistant(
                "assistant-large",
                vec![ContentBlock::Text {
                    text: final_text.clone(),
                }],
            ),
            final_text,
        }),
    }
}

fn large_final_text() -> String {
    "large-output ".repeat(256)
}

async fn read_stored_output(path: &str) -> ToolOutput {
    let bytes = tokio::fs::read(path)
        .await
        .expect("stored output should be readable");
    serde_json::from_slice(&bytes).expect("stored output should decode")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    dir.push(format!("noloong-subagent-tools-{name}-{unique}"));
    dir
}
