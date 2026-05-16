use noloong_agent::{
    AgentManifest, AgentSession, BuiltInToolName, ManifestPatch,
    interaction::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentRuntimeProfile, AgentSessionCreateRequest,
        AgentSessionRecord, AgentSessionRegistry, AgentSessionRegistryStore,
        DISPLAY_EVENT_NOTIFICATION, InMemoryAgentSessionRegistryStore, InteractionCapabilityPolicy,
        InteractionControlHandler, InteractionError, InteractionFuture,
        InteractionProfileDescriptor, JsonRpcHandler, RAW_EVENT_NOTIFICATION, serve_jsonrpc,
    },
    process::StartCommandRequest,
};
use noloong_agent_core::{
    AgentMessage, AgentRuntime, AgentState, BoxFuture, CancellationToken, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, QueueMode, RunStatus, StopReason, ToolCall,
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
async fn interaction_control_grants_are_scoped_to_jsonrpc_connection() {
    let handler = test_handler("default").await;
    let websocket_like_handler = handler.connection_handler();
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_reader, server_writer) = tokio::io::split(server);
    let server = tokio::spawn(serve_jsonrpc(
        server_reader,
        server_writer,
        websocket_like_handler,
    ));
    let (client_reader, mut client_writer) = tokio::io::split(client);
    let mut lines = BufReader::new(client_reader).lines();

    write_rpc(
        &mut client_writer,
        rpc(
            1,
            "initialize",
            json!({
                "name": "websocket-client",
                "requestedAuthority": ["agent.run"],
                "requestedUx": {"displayEvents": true, "streamText": true}
            }),
        ),
    )
    .await;
    assert_eq!(
        read_message(&mut lines).await["result"]["grant"]["ux"]["displayEvents"],
        true
    );
    write_rpc(
        &mut client_writer,
        rpc(2, "session/create", json!({"sessionId": "root"})),
    )
    .await;
    assert_eq!(
        read_message(&mut lines).await["result"]["sessionId"],
        "root"
    );

    let control_messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                10,
                "initialize",
                json!({
                    "name": "http-control-client",
                    "requestedAuthority": ["agent.run"],
                    "requestedUx": {}
                }),
            ),
            rpc(11, "shutdown", json!({})),
        ],
    )
    .await;
    assert_eq!(
        response(&control_messages, 10)["result"]["grant"]["ux"]["displayEvents"],
        false
    );

    write_rpc(
        &mut client_writer,
        rpc(3, "display/subscribe", json!({"sessionId": "root"})),
    )
    .await;
    assert!(
        read_message(&mut lines).await["result"]["subscriptionId"]
            .as_str()
            .is_some_and(|value| value.starts_with("subscription-"))
    );

    write_rpc(&mut client_writer, rpc(4, "shutdown", json!({}))).await;
    assert_eq!(read_message(&mut lines).await["result"]["ok"], true);
    drop(client_writer);
    server.await.unwrap().unwrap();
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
async fn interaction_control_manages_goal_lifecycle() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "goal-client",
                    "requestedAuthority": ["goal.manage"]
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(
                3,
                "goal/set",
                json!({
                    "sessionId": "root",
                    "objective": "ship goal support",
                    "tokenBudget": 100
                }),
            ),
            rpc(4, "goal/get", json!({"sessionId": "root"})),
            rpc(5, "goal/pause", json!({"sessionId": "root"})),
            rpc(6, "goal/resume", json!({"sessionId": "root"})),
            rpc(
                7,
                "goal/update",
                json!({
                    "sessionId": "root",
                    "status": "achieved",
                    "summary": "done",
                    "evidence": "test"
                }),
            ),
            rpc(8, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 3)["result"]["status"], "pursuing");
    assert_eq!(
        response(&messages, 4)["result"]["objective"],
        "ship goal support"
    );
    assert_eq!(response(&messages, 5)["result"]["status"], "paused");
    assert_eq!(response(&messages, 6)["result"]["status"], "pursuing");
    assert_eq!(response(&messages, 7)["result"]["status"], "achieved");
    assert_eq!(
        response(&messages, 7)["result"]["lastAudit"]["summary"],
        "done"
    );
}

#[tokio::test]
async fn interaction_control_creates_and_fires_automation() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "automation-client",
                    "requestedAuthority": ["automation.manage"]
                }),
            ),
            rpc(2, "session/create", json!({"sessionId": "root"})),
            rpc(
                3,
                "automation/create",
                json!({
                    "automationId": "auto-1",
                    "target": {"type": "existing_session", "sessionId": "root"},
                    "trigger": {
                        "type": "time",
                        "schedule": {"type": "once", "atMs": 4102444800000u64}
                    },
                    "prompt": {"type": "text", "text": "automation ping"}
                }),
            ),
            rpc(4, "automation/list", json!({})),
            rpc(5, "automation/fire", json!({"automationId": "auto-1"})),
            rpc(6, "session/get", json!({"sessionId": "root"})),
            rpc(7, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 3)["result"]["automationId"], "auto-1");
    assert_eq!(
        response(&messages, 4)["result"][0]["automationId"],
        "auto-1"
    );
    assert!(response(&messages, 5)["result"]["lastFiredAtMs"].is_number());
    assert_eq!(response(&messages, 6)["result"]["status"], "completed");
    assert_eq!(
        response(&messages, 6)["result"]["state"]["messages"][0]["metadata"]["source"]["type"],
        "automation"
    );
}

#[tokio::test]
async fn interaction_control_validates_automation_inputs_and_status() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(
                1,
                "initialize",
                json!({
                    "name": "automation-client",
                    "requestedAuthority": ["automation.manage"]
                }),
            ),
            rpc(
                2,
                "automation/create",
                json!({
                    "automationId": "bad-target",
                    "target": {"type": "existing_session", "sessionId": "missing"},
                    "trigger": {
                        "type": "time",
                        "schedule": {"type": "once", "atMs": 4102444800000u64}
                    },
                    "prompt": {"type": "text", "text": "automation ping"}
                }),
            ),
            rpc(
                3,
                "automation/create",
                json!({
                    "automationId": "bad-trigger",
                    "target": {"type": "new_session", "profileId": "default"},
                    "trigger": {
                        "type": "time",
                        "schedule": {"type": "interval", "intervalSeconds": 0}
                    },
                    "prompt": {"type": "text", "text": "automation ping"}
                }),
            ),
            rpc(4, "session/create", json!({"sessionId": "root"})),
            rpc(
                5,
                "automation/create",
                json!({
                    "automationId": "paused-auto",
                    "target": {"type": "existing_session", "sessionId": "root"},
                    "trigger": {
                        "type": "time",
                        "schedule": {"type": "once", "atMs": 4102444800000u64}
                    },
                    "prompt": {"type": "text", "text": "automation ping"}
                }),
            ),
            rpc(
                6,
                "automation/update",
                json!({"automationId": "paused-auto", "status": "paused"}),
            ),
            rpc(7, "automation/fire", json!({"automationId": "paused-auto"})),
            rpc(8, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 2)["error"]["code"], -32072);
    assert_eq!(response(&messages, 3)["error"]["code"], -32602);
    assert_eq!(response(&messages, 6)["result"]["status"], "paused");
    assert_eq!(response(&messages, 7)["error"]["code"], -32602);
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
async fn interaction_control_queue_changes_are_persisted() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        Arc::clone(&store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());
    let queued_message =
        serde_json::to_value(AgentMessage::user("queued-user", "queued input")).unwrap();

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
                "queue/set_mode",
                json!({"sessionId": "root", "queue": "steering", "mode": "all"}),
            ),
            rpc(
                4,
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
            rpc(5, "shutdown", json!({})),
        ],
    )
    .await;

    assert!(response(&messages, 4).get("result").is_some());
    let record = store.get("root").await.unwrap().unwrap();
    assert_eq!(record.queues.steering.mode, QueueMode::All);
    assert_eq!(record.queues.steering.messages[0].message.id, "queued-user");
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
        serde_json::json!({"source": "custom", "prompt": "Updated prompt."})
    );
    assert_eq!(
        response(&messages, 6)["result"]["manifest"]["systemPrompt"],
        serde_json::json!({"source": "custom", "prompt": "Updated prompt."})
    );
}

#[tokio::test]
async fn interaction_control_reads_resolved_system_prompt() {
    let handler = test_handler("default").await;

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(1, "initialize", json!({"name": "manifest-client"})),
            rpc(
                2,
                "session/create",
                json!({
                    "sessionId": "root",
                    "manifestPatches": [
                        {
                            "op": "upsert_system_prompt_addition",
                            "addition": {
                                "id": "interaction.test",
                                "text": "Current interaction channel: test client."
                            }
                        }
                    ]
                }),
            ),
            rpc(
                3,
                "manifest/system_prompt/get",
                json!({"sessionId": "root"}),
            ),
            rpc(4, "shutdown", json!({})),
        ],
    )
    .await;

    let result = &response(&messages, 3)["result"];
    assert_eq!(result["source"], "built_in");
    assert_eq!(result["configuredProfile"], "auto");
    assert_eq!(result["resolvedProfile"], "general");
    assert_eq!(
        result["additions"][0],
        json!({
            "id": "interaction.test",
            "text": "Current interaction channel: test client.",
            "enabled": true
        })
    );
    assert_eq!(result["enabledAdditionIds"], json!(["interaction.test"]));
    assert!(
        result["effectiveText"]
            .as_str()
            .unwrap()
            .contains("Current interaction channel: test client.")
    );
}

#[tokio::test]
async fn interaction_control_manifest_apply_is_persisted() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        Arc::clone(&store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
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
            prompt: "Persisted prompt.".into(),
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
            rpc(
                2,
                "manifest/proposals/approve",
                json!({"sessionId": "root", "proposalId": proposal.proposal_id}),
            ),
            rpc(3, "manifest/apply_approved", json!({"sessionId": "root"})),
            rpc(4, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(
        response(&messages, 3)["result"]["appliedProposalIds"][0],
        proposal.proposal_id
    );
    let record = store.get("root").await.unwrap().unwrap();
    assert_eq!(
        record.manifest.effective_system_prompt(),
        "Persisted prompt."
    );
}

#[tokio::test]
async fn interaction_control_get_reports_interrupted_session() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = control_record("root");
    record.state.status = RunStatus::Running;
    record.state.active_phase = Some("model_stream".into());
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        Arc::clone(&store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let handler =
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all());

    let messages = run_jsonrpc(
        handler,
        vec![
            rpc(1, "initialize", json!({"name": "session-client"})),
            rpc(2, "session/get", json!({"sessionId": "root"})),
            rpc(3, "shutdown", json!({})),
        ],
    )
    .await;

    assert_eq!(response(&messages, 2)["result"]["status"], "failed");
    assert_eq!(
        store.get("root").await.unwrap().unwrap().state.status,
        RunStatus::Failed
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
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_reader, server_writer) = tokio::io::split(server);
    let server = tokio::spawn(serve_jsonrpc(server_reader, server_writer, handler));
    let (client_reader, mut client_writer) = tokio::io::split(client);
    let mut lines = BufReader::new(client_reader).lines();

    write_rpc(
        &mut client_writer,
        rpc(
            1,
            "initialize",
            json!({
                "name": "approval-client",
                "requestedAuthority": ["agent.run", "approval.resolve"]
            }),
        ),
    )
    .await;
    read_message(&mut lines).await;

    write_rpc(
        &mut client_writer,
        rpc(
            2,
            "agent/prompt",
            json!({
                "sessionId": "root",
                "input": {"type": "text", "text": "run command"}
            }),
        ),
    )
    .await;
    let prompt_response = read_message(&mut lines).await;
    assert_eq!(prompt_response["result"]["status"], "paused");

    write_rpc(
        &mut client_writer,
        rpc(3, "approval/list", json!({"sessionId": "root"})),
    )
    .await;
    let list_response = read_message(&mut lines).await;
    let approvals = list_response["result"]
        .as_object()
        .expect("approval/list result is an object");
    let (approval_id, approval) = approvals
        .iter()
        .next()
        .expect("approval/list returns one pending approval");
    assert_eq!(approval["approvalId"], approval_id.as_str());

    write_rpc(
        &mut client_writer,
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
    )
    .await;
    let resolve_response = read_message(&mut lines).await;
    assert_eq!(resolve_response["result"]["status"], "completed");

    write_rpc(&mut client_writer, rpc(5, "shutdown", json!({}))).await;
    read_message(&mut lines).await;
    drop(client_writer);
    server.await.unwrap().unwrap();
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

async fn write_rpc<W>(writer: &mut W, request: Value)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let line = serde_json::to_vec(&request).unwrap();
    writer.write_all(&line).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
}

async fn read_message<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Value
where
    R: tokio::io::AsyncRead + Unpin,
{
    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap()
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

fn control_record(session_id: &str) -> AgentSessionRecord {
    AgentSessionRecord {
        schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION,
        session_id: session_id.into(),
        profile_id: "default".into(),
        parent_session_id: None,
        role: None,
        manifest: AgentManifest::default(),
        state: AgentState {
            run_id: Some("stored-run".into()),
            status: RunStatus::Completed,
            messages: vec![AgentMessage::user("stored-user", "hello")],
            completed_turns: 1,
            ..AgentState::default()
        },
        queues: Default::default(),
        metadata: Map::new(),
        created_at_ms: 1,
        updated_at_ms: 2,
    }
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
