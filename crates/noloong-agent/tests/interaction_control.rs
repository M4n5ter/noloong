use noloong_agent::{
    AgentManifest, AgentSession, BuiltInToolName, ManifestPatch,
    interaction::{
        AgentRuntimeProfile, AgentSessionCreateRequest, AgentSessionRegistry,
        DISPLAY_EVENT_NOTIFICATION, InteractionCapabilityPolicy, InteractionControlHandler,
        InteractionError, InteractionFuture, InteractionProfileDescriptor, RAW_EVENT_NOTIFICATION,
        serve_jsonrpc,
    },
    process::StartCommandRequest,
};
use noloong_agent_core::{
    AgentRuntime, BoxFuture, CancellationToken, ModelProvider, ModelRequest, ModelStreamEvent,
    ModelStreamSink, StopReason, ToolCall,
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn interaction_control_initializes_and_lists_profiles() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "test-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {"displayEvents": true, "streamText": true}
                }),
            ),
            rpc(2, "profile/list", json!({})),
            rpc(3, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(
        response(&messages, 1)["result"]["grant"]["authority"],
        json!(["agent.run"])
    );
    assert_eq!(
        response(&messages, 1)["result"]["profiles"][0]["profileId"],
        "default"
    );
    assert_eq!(response(&messages, 2)["result"][0]["profileId"], "default");
}

#[tokio::test]
async fn interaction_control_prompts_and_emits_raw_and_display_events() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "test-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {
                        "rawEvents": true,
                        "displayEvents": true,
                        "streamText": true,
                        "editMessage": true
                    }
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(3, "event/subscribe", json!({"sessionId": "root"})),
            rpc(
                4,
                "display/subscribe",
                json!({
                    "sessionId": "root",
                    "ux": {"displayEvents": true, "streamText": true, "editMessage": true}
                }),
            ),
            rpc(
                5,
                "agent/prompt",
                json!({
                    "sessionId": "root",
                    "input": {"type": "text", "text": "hello"}
                }),
            ),
            rpc(6, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 5)["result"]["status"], "completed");
    assert!(messages.iter().any(|message| {
        message["method"] == RAW_EVENT_NOTIFICATION
            && message["params"]["event"]["kind"]["type"] == "run_started"
    }));
    assert!(messages.iter().any(|message| {
        message["method"] == DISPLAY_EVENT_NOTIFICATION
            && message["params"]["event"]["type"] == "assistant_message_delta"
    }));
    assert!(messages.iter().any(|message| {
        message["method"] == DISPLAY_EVENT_NOTIFICATION
            && message["params"]["event"]["type"] == "assistant_message_final"
    }));
}

#[tokio::test]
async fn interaction_control_display_can_be_final_only_and_bounded() {
    let handler = InteractionControlHandler::new(
        AgentSessionRegistry::new(model_profile(
            "long-text",
            Arc::new(StaticTextModel {
                text: "abcdefghijklmnopqrstuvwxyz0123456789".into(),
            }),
        ))
        .unwrap(),
        InteractionCapabilityPolicy::allow_all(),
    );

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "final-only-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {
                        "displayEvents": true,
                        "streamText": false,
                        "maxMessageBytes": 30
                    }
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(
                3,
                "display/subscribe",
                json!({
                    "sessionId": "root",
                    "ux": {
                        "displayEvents": true,
                        "streamText": false,
                        "maxMessageBytes": 30
                    }
                }),
            ),
            rpc(
                4,
                "agent/prompt",
                json!({
                    "sessionId": "root",
                    "input": {"type": "text", "text": "hello"}
                }),
            ),
            rpc(5, "shutdown", json!({})),
        ],
    )
    .await;

    assert!(!messages.iter().any(|message| {
        message["method"] == DISPLAY_EVENT_NOTIFICATION
            && message["params"]["event"]["type"] == "assistant_message_delta"
    }));
    let final_event = messages
        .iter()
        .find(|message| {
            message["method"] == DISPLAY_EVENT_NOTIFICATION
                && message["params"]["event"]["type"] == "assistant_message_final"
        })
        .expect("final display event should exist");
    assert_eq!(final_event["params"]["event"]["truncated"], true);
    assert!(
        final_event["params"]["event"]["message"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .len()
            <= 30
    );
}

#[tokio::test]
async fn interaction_control_raw_event_unsubscribe_stops_notifications() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "raw-event-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {"rawEvents": true}
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(3, "event/subscribe", json!({"sessionId": "root"})),
            rpc(
                4,
                "event/unsubscribe",
                json!({"subscriptionId": "subscription-1"}),
            ),
            rpc(
                5,
                "agent/prompt",
                json!({
                    "sessionId": "root",
                    "input": {"type": "text", "text": "hello"}
                }),
            ),
            rpc(6, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 4)["result"]["unsubscribed"], true);
    assert!(
        !messages
            .iter()
            .any(|message| message["method"] == RAW_EVENT_NOTIFICATION)
    );
}

#[tokio::test]
async fn interaction_control_event_subscriptions_require_granted_ux() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "no-event-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {}
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(3, "event/subscribe", json!({"sessionId": "root"})),
            rpc(4, "display/subscribe", json!({"sessionId": "root"})),
            rpc(5, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 3)["error"]["code"], -32070);
    assert_eq!(
        response(&messages, 3)["error"]["data"]["requiredCapability"],
        "rawEvents"
    );
    assert_eq!(response(&messages, 4)["error"]["code"], -32070);
    assert_eq!(
        response(&messages, 4)["error"]["data"]["requiredCapability"],
        "displayEvents"
    );
}

#[tokio::test]
async fn interaction_control_gates_sensitive_methods() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "limited-client",
                    "requestedAuthority": ["agent.run"]
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(3, "session/delete", json!({"sessionId": "root"})),
            rpc(4, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 3)["error"]["code"], -32070);
    assert_eq!(
        response(&messages, 3)["error"]["data"]["requiredCapability"],
        "session.delete"
    );
}

#[tokio::test]
async fn interaction_control_subagent_spawn_requires_capability() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "limited-client",
                    "requestedAuthority": []
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(
                3,
                "subagent/spawn",
                json!({"parentSessionId": "root", "role": "worker"}),
            ),
            rpc(4, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 3)["error"]["code"], -32070);
    assert_eq!(
        response(&messages, 3)["error"]["data"]["requiredCapability"],
        "subagent.spawn"
    );
}

#[tokio::test]
async fn interaction_control_edits_agent_queues() {
    let handler = test_handler("default").await;
    let queued_message = serde_json::to_value(noloong_agent_core::AgentMessage::user(
        "queued-user",
        "queued input",
    ))
    .unwrap();

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "queue-client",
                    "requestedAuthority": ["agent.queue"]
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(
                3,
                "queue/edit",
                json!({
                    "sessionId": "root",
                    "queue": "steering",
                    "messages": [{
                        "message": queued_message,
                        "intent": "user_input"
                    }]
                }),
            ),
            rpc(
                4,
                "queue/list",
                json!({"sessionId": "root", "queue": "steering"}),
            ),
            rpc(5, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 4)["result"][0]["intent"], "user_input");
    assert_eq!(
        response(&messages, 4)["result"][0]["message"]["id"],
        "queued-user"
    );
}

#[tokio::test]
async fn interaction_control_lists_approves_and_applies_manifest_proposals() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let registered = registry.get("root").await.unwrap().unwrap();
    let proposal = registered
        .session()
        .proposal_store()
        .record_pending_proposal(ManifestPatch::ReplaceSystemPrompt {
            prompt: "Updated prompt.".into(),
        })
        .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "manifest-client",
                    "requestedAuthority": ["manifest.apply"]
                }),
            ),
            rpc(2, "manifest/proposals/list", json!({"sessionId": "root"})),
            rpc(
                3,
                "manifest/proposals/approve",
                json!({"sessionId": "root", "proposalId": proposal.proposal_id}),
            ),
            rpc(4, "manifest/apply_approved", json!({"sessionId": "root"})),
            rpc(5, "manifest/get", json!({"sessionId": "root"})),
            rpc(6, "session/get", json!({"sessionId": "root"})),
            rpc(7, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(
        response(&messages, 2)["result"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        response(&messages, 4)["result"]["appliedProposalIds"][0],
        proposal.proposal_id
    );
    assert_eq!(
        response(&messages, 5)["result"]["systemPrompt"],
        "Updated prompt."
    );
    assert_eq!(
        response(&messages, 6)["result"]["manifest"]["systemPrompt"],
        "Updated prompt."
    );
}

#[tokio::test]
async fn interaction_control_lists_reads_and_controls_processes() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let registered = registry.get("root").await.unwrap().unwrap();
    let snapshot = registered
        .session()
        .process_manager()
        .start(StartCommandRequest {
            command: "cat".into(),
            shell: None,
            cwd: None,
            env: BTreeMap::new(),
            pipe_stdin: true,
            max_spool_bytes: None,
            foreground_wait_ms: None,
        })
        .await
        .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "process-client",
                    "requestedAuthority": ["process.control"]
                }),
            ),
            rpc(2, "process/list", json!({"sessionId": "root"})),
            rpc(
                3,
                "process/write",
                json!({"sessionId": "root", "jobId": snapshot.job_id, "text": "hello\n"}),
            ),
            rpc(
                4,
                "process/read",
                json!({"sessionId": "root", "jobId": snapshot.job_id, "waitMs": 500}),
            ),
            rpc(
                5,
                "process/terminate",
                json!({"sessionId": "root", "jobId": snapshot.job_id}),
            ),
            rpc(
                6,
                "process/wait",
                json!({"sessionId": "root", "jobId": snapshot.job_id, "timeoutMs": 500}),
            ),
            rpc(7, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(
        response(&messages, 2)["result"][0]["jobId"],
        snapshot.job_id
    );
    assert_eq!(
        response(&messages, 4)["result"]["chunks"][0]["text"],
        "hello\n"
    );
    assert_eq!(response(&messages, 6)["result"]["timedOut"], false);
}

#[tokio::test]
async fn interaction_control_process_control_requires_capability() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let registered = registry.get("root").await.unwrap().unwrap();
    let snapshot = registered
        .session()
        .process_manager()
        .start(StartCommandRequest {
            command: "printf readonly".into(),
            shell: None,
            cwd: None,
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(1_000),
        })
        .await
        .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "readonly-process-client",
                    "requestedAuthority": []
                }),
            ),
            rpc(2, "process/list", json!({"sessionId": "root"})),
            rpc(
                3,
                "process/wait",
                json!({"sessionId": "root", "jobId": snapshot.job_id, "timeoutMs": 10}),
            ),
            rpc(4, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(
        response(&messages, 2)["result"][0]["jobId"],
        snapshot.job_id
    );
    assert_eq!(response(&messages, 3)["error"]["code"], -32070);
    assert_eq!(
        response(&messages, 3)["error"]["data"]["requiredCapability"],
        "process.control"
    );
}

#[tokio::test]
async fn interaction_control_lists_and_resolves_tool_approvals() {
    let registry = AgentSessionRegistry::new(model_profile(
        "approval",
        Arc::new(HostExecApprovalModel::default()),
    ))
    .unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
            manifest: Some(
                AgentManifest::default()
                    .with_enabled_tool(BuiltInToolName::HostExecStart)
                    .with_file_edit_tool_policy(noloong_agent::FileEditToolPolicy::Disabled),
            ),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());
    let approval_id = "approval-run-1-1-host-exec-start-test-0";

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "approval-client",
                    "requestedAuthority": ["agent.run", "approval.resolve"]
                }),
            ),
            rpc(
                2,
                "agent/prompt",
                json!({
                    "sessionId": "root",
                    "input": {"type": "text", "text": "run command"}
                }),
            ),
            rpc(3, "approval/list", json!({"sessionId": "root"})),
            rpc(
                4,
                "approval/resolve",
                json!({
                    "sessionId": "root",
                    "approvalId": approval_id,
                    "decision": {
                        "outcome": "allow",
                        "reason": "test approval",
                        "approver": "test",
                        "metadata": {}
                    }
                }),
            ),
            rpc(5, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 2)["result"]["status"], "paused");
    assert_eq!(
        response(&messages, 3)["result"][approval_id]["approvalId"],
        approval_id
    );
    assert_eq!(response(&messages, 4)["result"]["status"], "completed");
}

async fn test_handler(profile_id: &str) -> InteractionControlHandler {
    InteractionControlHandler::new(
        AgentSessionRegistry::new(text_profile(profile_id)).unwrap(),
        InteractionCapabilityPolicy::allow_all(),
    )
}

async fn run_jsonrpc(handler: InteractionControlHandler, requests: Vec<Value>) -> Vec<Value> {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_reader, server_writer) = tokio::io::split(server);
    let server = tokio::spawn(serve_jsonrpc(server_reader, server_writer, handler));
    let (client_reader, mut client_writer) = tokio::io::split(client);

    for request in requests {
        let line = serde_json::to_vec(&request).unwrap();
        client_writer.write_all(&line).await.unwrap();
        client_writer.write_all(b"\n").await.unwrap();
    }
    client_writer.flush().await.unwrap();

    let mut lines = BufReader::new(client_reader).lines();
    let mut messages = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        let message = serde_json::from_str::<Value>(&line).unwrap();
        let shutdown = message
            .get("result")
            .and_then(|result| result.get("ok"))
            .and_then(Value::as_bool)
            == Some(true);
        messages.push(message);
        if shutdown {
            break;
        }
    }
    drop(client_writer);
    server.await.unwrap().unwrap();
    messages
}

fn response(messages: &[Value], id: i64) -> &Value {
    messages
        .iter()
        .find(|message| message.get("id").and_then(Value::as_i64) == Some(id))
        .expect("response should exist")
}

fn rpc(id: i64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

fn text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    model_profile(profile_id, Arc::new(TextModel))
}

fn model_profile(profile_id: &str, model: Arc<dyn ModelProvider>) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: InteractionProfileDescriptor {
            profile_id: profile_id.into(),
            display_name: profile_id.into(),
            description: None,
            default_manifest_patches: Vec::new(),
            metadata: Map::new(),
        },
        model,
    })
}

struct TestProfile {
    descriptor: InteractionProfileDescriptor,
    model: Arc<dyn ModelProvider>,
}

impl AgentRuntimeProfile for TestProfile {
    fn descriptor(&self) -> InteractionProfileDescriptor {
        self.descriptor.clone()
    }

    fn build_runtime<'a>(
        &'a self,
        session: &'a AgentSession,
        _manifest: &'a AgentManifest,
    ) -> InteractionFuture<'a, AgentRuntime> {
        Box::pin(async move {
            session
                .runtime_builder()
                .with_model_provider(Arc::clone(&self.model))
                .build()
                .map_err(InteractionError::from)
        })
    }
}

#[derive(Default)]
struct HostExecApprovalModel {
    calls: AtomicU64,
}

impl ModelProvider for HostExecApprovalModel {
    fn id(&self) -> &str {
        "host-exec-approval"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "approval-stream-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: ToolCall {
                            id: "host-exec-start-test".into(),
                            name: BuiltInToolName::HostExecStart.as_str().into(),
                            arguments: json!({
                                "command": "printf approved",
                                "shell": "sh",
                                "cwd": ".",
                                "pipeStdin": false,
                                "foregroundWaitMs": 1000
                            }),
                        },
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "approval-stream-2".into(),
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
                    stream_id: "test-stream".into(),
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

struct StaticTextModel {
    text: String,
}

impl ModelProvider for StaticTextModel {
    fn id(&self) -> &str {
        "static-text"
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
                    stream_id: "static-text-stream".into(),
                },
                ModelStreamEvent::TextDelta {
                    text: self.text.clone(),
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
