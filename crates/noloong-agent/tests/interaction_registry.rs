use noloong_agent::{
    AgentManifest, AgentSession, BuiltInToolName, ManifestPatch,
    interaction::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentRuntimeProfile, AgentSessionCreateRequest,
        AgentSessionDeleteOptions, AgentSessionListFilter, AgentSessionQueueSnapshot,
        AgentSessionQueueState, AgentSessionQueuedMessage, AgentSessionQueuedMessageIntent,
        AgentSessionRecord, AgentSessionRegistry, AgentSessionRegistryStore,
        INTERACTION_ERROR_BUSY, INTERACTION_ERROR_NOT_FOUND, InMemoryAgentSessionRegistryStore,
        InteractionError, InteractionFuture, InteractionProfileDescriptor,
        InteractionSessionStatus, SubagentSpawnRequest,
    },
};
use noloong_agent_core::{
    AgentCoreError, AgentMessage, AgentRuntime, AgentState, BoxFuture, CancellationToken,
    EventStore, InMemoryEventStore, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink,
    QueueMode, RunStatus, StopReason, ToolApprovalRequest, ToolApprovalRequestSpec,
    ToolApprovalResolution, ToolCall, ToolPermissionDecision, ToolPermissionOutcome,
};
use serde_json::Map;
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn interaction_registry_creates_lists_and_gets_sessions() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();

    let created = registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    assert_eq!(created.session_id, "root");
    assert_eq!(created.profile_id, "default");
    assert_eq!(created.status, InteractionSessionStatus::Idle);

    let listed = registry
        .list(AgentSessionListFilter::default())
        .await
        .unwrap();
    assert_eq!(listed, vec![created.clone()]);

    let fetched = registry.get_descriptor("root").await.unwrap().unwrap();
    assert_eq!(fetched, created);
}

#[test]
fn snapshot_record_serde_round_trips() {
    let record = AgentSessionRecord {
        schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION,
        session_id: "root/with space".into(),
        profile_id: "default".into(),
        parent_session_id: Some("parent".into()),
        role: Some("worker".into()),
        manifest: AgentManifest {
            system_prompt: "Persisted prompt".into(),
            ..AgentManifest::default()
        },
        state: AgentState {
            status: RunStatus::Completed,
            messages: vec![AgentMessage::user("user-1", "hello")],
            completed_turns: 3,
            ..AgentState::default()
        },
        queues: AgentSessionQueueSnapshot {
            steering: AgentSessionQueueState {
                mode: QueueMode::All,
                messages: vec![AgentSessionQueuedMessage {
                    message: AgentMessage::user("steer-1", "background"),
                    intent: AgentSessionQueuedMessageIntent::Observation,
                }],
            },
            follow_up: AgentSessionQueueState {
                mode: QueueMode::OneAtATime,
                messages: vec![AgentSessionQueuedMessage {
                    message: AgentMessage::user("follow-1", "next"),
                    intent: AgentSessionQueuedMessageIntent::UserInput,
                }],
            },
        },
        metadata: [("kind".into(), json!("root"))].into_iter().collect(),
        created_at_ms: 11,
        updated_at_ms: 22,
    };

    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(value["schemaVersion"], AGENT_SESSION_RECORD_SCHEMA_VERSION);
    assert_eq!(value["sessionId"], "root/with space");
    assert_eq!(value["queues"]["steering"]["mode"], "all");

    let decoded: AgentSessionRecord = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, record);
}

#[test]
fn snapshot_preserves_queues_and_state() {
    let record = stored_record("queued");
    assert_eq!(record.state.messages[0].id, "stored-user");
    assert_eq!(record.queues.steering.mode, QueueMode::All);
    assert_eq!(
        record.queues.steering.messages[0].message.id,
        "stored-steer"
    );
    assert_eq!(
        record.queues.follow_up.messages[0].intent,
        AgentSessionQueuedMessageIntent::UserInput
    );
}

#[tokio::test]
async fn store_insert_rejects_duplicate_session_id() {
    let store = InMemoryAgentSessionRegistryStore::default();
    store.insert(stored_record("same")).await.unwrap();

    let error = store
        .insert(stored_record("same"))
        .await
        .expect_err("duplicate insert should fail");

    assert!(error.message.contains("session already exists"));
}

#[tokio::test]
async fn store_save_requires_existing_session() {
    let store = InMemoryAgentSessionRegistryStore::default();

    let error = store
        .save(stored_record("missing"))
        .await
        .expect_err("save should not create missing sessions");

    assert_eq!(error.code, INTERACTION_ERROR_NOT_FOUND);
}

#[tokio::test]
async fn interaction_registry_applies_profile_default_manifest_patches() {
    let mut descriptor = descriptor("patched");
    descriptor.default_manifest_patches = vec![
        ManifestPatch::ReplaceSystemPrompt {
            prompt: "Use the runtime profile prompt.".into(),
        },
        ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        },
    ];
    let registry = AgentSessionRegistry::new(Arc::new(TestProfile {
        descriptor,
        model: Arc::new(TextModel),
        build_delay_ms: 0,
        build_count: None,
    }))
    .unwrap();

    let created = registry
        .create_session(AgentSessionCreateRequest::default())
        .await
        .unwrap();

    assert_eq!(
        created.manifest.system_prompt,
        "Use the runtime profile prompt."
    );
    assert!(
        created
            .manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
}

#[tokio::test]
async fn interaction_registry_reports_unknown_profile() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();

    let error = registry
        .create_session(AgentSessionCreateRequest {
            profile_id: Some("missing".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .expect_err("unknown profile should fail");

    assert_eq!(error.code, INTERACTION_ERROR_NOT_FOUND);
}

#[tokio::test]
async fn registry_lists_unloaded_stored_sessions() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    store.insert(stored_record("stored")).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let listed = registry
        .list(AgentSessionListFilter::default())
        .await
        .unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, "stored");
    assert_eq!(listed[0].state.messages[0].id, "stored-user");
}

#[tokio::test]
async fn registry_get_descriptor_does_not_restore_runtime() {
    let build_count = Arc::new(AtomicU64::new(0));
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    store.insert(stored_record("stored")).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![counting_text_profile("default", Arc::clone(&build_count))],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let descriptor = registry
        .get_descriptor("stored")
        .await
        .unwrap()
        .expect("stored descriptor exists");

    assert_eq!(descriptor.session_id, "stored");
    assert_eq!(build_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn registry_filters_unloaded_sessions() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut child = stored_record("child");
    child.parent_session_id = Some("parent".into());
    child.profile_id = "default".into();
    store.insert(stored_record("root")).await.unwrap();
    store.insert(child.clone()).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let listed = registry
        .list(AgentSessionListFilter {
            parent_session_id: Some("parent".into()),
            profile_id: Some("default".into()),
            status: Some(InteractionSessionStatus::Completed),
        })
        .await
        .unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, "child");
}

#[tokio::test]
async fn registry_lazy_restores_session_for_prompt() {
    let build_count = Arc::new(AtomicU64::new(0));
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("stored");
    record.state = AgentState::default();
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![counting_text_profile("default", Arc::clone(&build_count))],
        Arc::clone(&store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let registered = registry
        .get("stored")
        .await
        .unwrap()
        .expect("session restores");
    registered.agent().prompt("hello").await.unwrap();

    assert_eq!(build_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        registry
            .get_descriptor("stored")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionSessionStatus::Completed
    );
}

#[tokio::test]
async fn registry_restore_missing_profile_is_structured_error() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("stored");
    record.profile_id = "missing".into();
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let descriptor = registry
        .get_descriptor("stored")
        .await
        .unwrap()
        .expect("read-only descriptor should not need the profile");
    assert_eq!(descriptor.profile_id, "missing");

    let error = match registry.get("stored").await {
        Ok(_) => panic!("live restore should need the runtime profile"),
        Err(error) => error,
    };
    assert_eq!(error.code, INTERACTION_ERROR_NOT_FOUND);
}

#[tokio::test]
async fn registry_rejects_concurrent_duplicate_restore() {
    let build_count = Arc::new(AtomicU64::new(0));
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    store.insert(stored_record("stored")).await.unwrap();
    let profile: Arc<dyn AgentRuntimeProfile> = Arc::new(TestProfile {
        descriptor: descriptor("default"),
        model: Arc::new(TextModel),
        build_delay_ms: 25,
        build_count: Some(Arc::clone(&build_count)),
    });
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![profile],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let (first, second) = tokio::join!(registry.get("stored"), registry.get("stored"));

    assert!(first.unwrap().is_some());
    assert!(second.unwrap().is_some());
    assert_eq!(build_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn registry_restore_uses_persisted_manifest() {
    let mut descriptor = descriptor("default");
    descriptor.default_manifest_patches = vec![ManifestPatch::ReplaceSystemPrompt {
        prompt: "Profile default prompt".into(),
    }];
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("stored");
    record.manifest.system_prompt = "Persisted prompt".into();
    store.insert(record).await.unwrap();
    let profile: Arc<dyn AgentRuntimeProfile> = Arc::new(TestProfile {
        descriptor,
        model: Arc::new(TextModel),
        build_delay_ms: 0,
        build_count: None,
    });
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![profile],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let restored = registry
        .get("stored")
        .await
        .unwrap()
        .expect("session restores");

    assert_eq!(
        restored.session().manifest().system_prompt,
        "Persisted prompt"
    );
}

#[tokio::test]
async fn registry_restore_replays_queue_snapshot() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    store.insert(stored_record("stored")).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let restored = registry
        .get("stored")
        .await
        .unwrap()
        .expect("session restores");

    assert_eq!(restored.agent().steering_queue_mode(), QueueMode::All);
    assert_eq!(
        restored.agent().queued_steering_messages()[0].message.id,
        "stored-steer"
    );
    assert_eq!(
        restored.agent().queued_follow_up_messages()[0].message.id,
        "stored-follow"
    );
}

#[tokio::test]
async fn registry_restore_marks_running_session_failed() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("running");
    record.state.status = RunStatus::Running;
    record.state.active_phase = Some("model_stream".into());
    record.state.last_error = None;
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        Arc::clone(&store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let descriptor = registry
        .get_descriptor("running")
        .await
        .unwrap()
        .expect("descriptor exists");

    assert_eq!(descriptor.status, InteractionSessionStatus::Failed);
    assert_eq!(descriptor.state.active_phase, None);
    assert!(
        descriptor
            .state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("interrupted")
    );
    assert_eq!(
        store.get("running").await.unwrap().unwrap().state.status,
        RunStatus::Failed
    );
}

#[tokio::test]
async fn registry_persists_state_after_prompt() {
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

    registry
        .get("root")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("hello")
        .await
        .unwrap();

    let record = store.get("root").await.unwrap().unwrap();
    assert_eq!(record.state.status, RunStatus::Completed);
    assert_eq!(record.state.messages.len(), 2);
}

#[tokio::test]
async fn registry_does_not_snapshot_model_deltas() {
    let store = Arc::new(CountingSaveStore::default());
    let profile: Arc<dyn AgentRuntimeProfile> = Arc::new(TestProfile {
        descriptor: descriptor("default"),
        model: Arc::new(ManyDeltaModel { deltas: 128 }),
        build_delay_ms: 0,
        build_count: None,
    });
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![profile],
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

    registry
        .get("root")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("hello")
        .await
        .unwrap();

    assert!(store.save_count() < 128);
}

#[tokio::test]
async fn registry_snapshot_save_error_fails_run() {
    let store = Arc::new(FailingSaveStore::default());
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
    store.fail_saves();

    let result = registry
        .get("root")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("hello")
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn registry_restore_paused_session_resume_fails_without_event_log() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("paused");
    record.state.status = RunStatus::Paused;
    record.state.pending_tool_approvals.insert(
        "approval-missing".into(),
        ToolApprovalRequest {
            approval_id: "approval-missing".into(),
            tool_call: ToolCall {
                id: "tool-call".into(),
                name: "unknown".into(),
                arguments: json!({}),
            },
            permissions: Vec::new(),
            hook_id: None,
            request: ToolApprovalRequestSpec {
                prompt: None,
                reason: None,
                expires_at_ms: None,
                metadata: json!({}),
            },
        },
    );
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let restored = registry
        .get("paused")
        .await
        .unwrap()
        .expect("paused session restores");
    let result = restored
        .agent()
        .resume_tool_approval(ToolApprovalResolution {
            approval_id: "approval-missing".into(),
            decision: ToolPermissionDecision {
                outcome: ToolPermissionOutcome::Allow,
                reason: Some("approved".into()),
                approver: Some("test".into()),
                metadata: json!({}),
            },
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn registry_restore_paused_session_can_resume_with_shared_event_store() {
    let registry_store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let event_store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let model = Arc::new(HostExecApprovalModel::default());
    let first_registry = AgentSessionRegistry::with_store(
        "approval",
        vec![shared_event_store_profile(
            "approval",
            Arc::clone(&model) as Arc<dyn ModelProvider>,
            Arc::clone(&event_store),
        )],
        Arc::clone(&registry_store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    first_registry
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
    first_registry
        .get("root")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("run command")
        .await
        .unwrap();
    assert_eq!(
        registry_store
            .get("root")
            .await
            .unwrap()
            .unwrap()
            .state
            .status,
        RunStatus::Paused
    );

    let second_registry = AgentSessionRegistry::with_store(
        "approval",
        vec![shared_event_store_profile(
            "approval",
            model as Arc<dyn ModelProvider>,
            event_store,
        )],
        registry_store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let restored = second_registry
        .get("root")
        .await
        .unwrap()
        .expect("paused session restores");

    restored
        .agent()
        .resume_tool_approval(ToolApprovalResolution {
            approval_id: "approval-run-1-1-host-exec-start-test-0".into(),
            decision: ToolPermissionDecision {
                outcome: ToolPermissionOutcome::Allow,
                reason: Some("approved".into()),
                approver: Some("test".into()),
                metadata: json!({}),
            },
        })
        .await
        .unwrap();

    assert_eq!(restored.agent().state().await.status, RunStatus::Completed);
}

#[tokio::test]
async fn registry_restore_preserves_paused_session() {
    let store = Arc::new(InMemoryAgentSessionRegistryStore::default());
    let mut record = stored_record("paused");
    record.state.status = RunStatus::Paused;
    record.state.active_phase = Some("tool_execute".into());
    store.insert(record).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let descriptor = registry
        .get_descriptor("paused")
        .await
        .unwrap()
        .expect("descriptor exists");

    assert_eq!(descriptor.status, InteractionSessionStatus::Paused);
    assert_eq!(
        descriptor.state.active_phase.as_deref(),
        Some("tool_execute")
    );
}

#[tokio::test]
async fn interaction_registry_rejects_concurrent_duplicate_session_id() {
    let registry = AgentSessionRegistry::new(slow_text_profile("default")).unwrap();
    let first = registry.create_session(AgentSessionCreateRequest {
        session_id: Some("same".into()),
        ..AgentSessionCreateRequest::default()
    });
    let second = registry.create_session(AgentSessionCreateRequest {
        session_id: Some("same".into()),
        ..AgentSessionCreateRequest::default()
    });

    let (first, second) = tokio::join!(first, second);
    let successes = [first.as_ref().is_ok(), second.as_ref().is_ok()]
        .into_iter()
        .filter(|success| *success)
        .count();

    assert_eq!(successes, 1);
    assert_eq!(
        registry
            .list(AgentSessionListFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn interaction_registry_spawns_subagent_with_parent_and_initial_prompt() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    let parent = registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent".into()),
            metadata: [("kind".into(), json!("root"))].into_iter().collect(),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    let child = registry
        .spawn_subagent(SubagentSpawnRequest {
            parent_session_id: parent.session_id.clone(),
            role: Some("researcher".into()),
            metadata: [("topic".into(), json!("storage"))].into_iter().collect(),
            initial_prompt: Some(AgentMessage::user("first-user-message", "hello")),
            ..SubagentSpawnRequest::default()
        })
        .await
        .unwrap();

    assert_eq!(child.parent_session_id.as_deref(), Some("parent"));
    assert_eq!(child.role.as_deref(), Some("researcher"));
    assert_eq!(child.profile_id, parent.profile_id);
    assert_eq!(child.metadata["topic"], "storage");
    assert_eq!(child.status, InteractionSessionStatus::Completed);
    assert_eq!(child.state.messages.len(), 2);
}

#[tokio::test]
async fn interaction_registry_filters_by_parent_profile_and_status() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let child = registry
        .spawn_subagent(SubagentSpawnRequest {
            parent_session_id: "parent".into(),
            role: Some("worker".into()),
            ..SubagentSpawnRequest::default()
        })
        .await
        .unwrap();

    let filtered = registry
        .list(AgentSessionListFilter {
            parent_session_id: Some("parent".into()),
            profile_id: Some("default".into()),
            status: Some(InteractionSessionStatus::Idle),
        })
        .await
        .unwrap();

    assert_eq!(filtered, vec![child]);
}

#[tokio::test]
async fn interaction_registry_requires_force_to_delete_running_session() {
    let registry = AgentSessionRegistry::new(blocking_profile("blocking")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("blocked".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let registered = registry.get("blocked").await.unwrap().unwrap();
    let agent = registered.agent().clone();
    let run = tokio::spawn(async move { agent.prompt("wait").await });

    wait_until_status(&registry, "blocked", InteractionSessionStatus::Running).await;

    let error = registry
        .delete_session("blocked", AgentSessionDeleteOptions::default())
        .await
        .expect_err("running session should require force");
    assert_eq!(error.code, INTERACTION_ERROR_BUSY);

    registry
        .delete_session("blocked", AgentSessionDeleteOptions { force_abort: true })
        .await
        .unwrap();
    assert!(registry.get("blocked").await.unwrap().is_none());

    let run_result = run.await.unwrap();
    assert!(matches!(run_result, Err(AgentCoreError::Aborted)));
}

async fn wait_until_status(
    registry: &AgentSessionRegistry,
    session_id: &str,
    status: InteractionSessionStatus,
) {
    for _ in 0..50 {
        let descriptor = registry
            .get_descriptor(session_id)
            .await
            .unwrap()
            .expect("session should exist");
        if descriptor.status == status {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("session did not reach status {status:?}");
}

fn text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(TextModel),
        build_delay_ms: 0,
        build_count: None,
    })
}

fn counting_text_profile(
    profile_id: &str,
    build_count: Arc<AtomicU64>,
) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(TextModel),
        build_delay_ms: 0,
        build_count: Some(build_count),
    })
}

fn slow_text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(TextModel),
        build_delay_ms: 25,
        build_count: None,
    })
}

fn blocking_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(BlockingModel),
        build_delay_ms: 0,
        build_count: None,
    })
}

fn shared_event_store_profile(
    profile_id: &str,
    model: Arc<dyn ModelProvider>,
    event_store: Arc<dyn EventStore>,
) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(SharedEventStoreProfile {
        descriptor: descriptor(profile_id),
        model,
        event_store,
    })
}

fn descriptor(profile_id: &str) -> InteractionProfileDescriptor {
    InteractionProfileDescriptor {
        profile_id: profile_id.into(),
        display_name: profile_id.into(),
        description: None,
        default_manifest_patches: Vec::new(),
        metadata: Map::new(),
    }
}

struct TestProfile {
    descriptor: InteractionProfileDescriptor,
    model: Arc<dyn ModelProvider>,
    build_delay_ms: u64,
    build_count: Option<Arc<AtomicU64>>,
}

struct SharedEventStoreProfile {
    descriptor: InteractionProfileDescriptor,
    model: Arc<dyn ModelProvider>,
    event_store: Arc<dyn EventStore>,
}

impl AgentRuntimeProfile for SharedEventStoreProfile {
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
                .with_event_store(Arc::clone(&self.event_store))
                .with_model_provider(Arc::clone(&self.model))
                .build()
                .map_err(InteractionError::from)
        })
    }
}

impl AgentRuntimeProfile for TestProfile {
    fn descriptor(&self) -> InteractionProfileDescriptor {
        self.descriptor.clone()
    }

    fn build_runtime<'a>(
        &'a self,
        _session: &'a AgentSession,
        _manifest: &'a AgentManifest,
    ) -> InteractionFuture<'a, AgentRuntime> {
        Box::pin(async move {
            if let Some(build_count) = &self.build_count {
                build_count.fetch_add(1, Ordering::SeqCst);
            }
            if self.build_delay_ms > 0 {
                sleep(Duration::from_millis(self.build_delay_ms)).await;
            }
            AgentRuntime::builder()
                .with_model_provider(Arc::clone(&self.model))
                .build()
                .map_err(InteractionError::from)
        })
    }
}

fn stored_record(session_id: &str) -> AgentSessionRecord {
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
            messages: vec![AgentMessage::user("stored-user", "stored hello")],
            completed_turns: 1,
            ..AgentState::default()
        },
        queues: AgentSessionQueueSnapshot {
            steering: AgentSessionQueueState {
                mode: QueueMode::All,
                messages: vec![AgentSessionQueuedMessage {
                    message: AgentMessage::user("stored-steer", "stored steering"),
                    intent: AgentSessionQueuedMessageIntent::Observation,
                }],
            },
            follow_up: AgentSessionQueueState {
                mode: QueueMode::OneAtATime,
                messages: vec![AgentSessionQueuedMessage {
                    message: AgentMessage::user("stored-follow", "stored follow up"),
                    intent: AgentSessionQueuedMessageIntent::UserInput,
                }],
            },
        },
        metadata: Map::new(),
        created_at_ms: 1,
        updated_at_ms: 2,
    }
}

#[derive(Default)]
struct CountingSaveStore {
    inner: InMemoryAgentSessionRegistryStore,
    saves: AtomicU64,
}

impl CountingSaveStore {
    fn save_count(&self) -> u64 {
        self.saves.load(Ordering::SeqCst)
    }
}

impl AgentSessionRegistryStore for CountingSaveStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        self.inner.insert(record)
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.inner.save(record)
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove(session_id)
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        self.inner.get(session_id)
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        self.inner.list()
    }
}

#[derive(Default)]
struct FailingSaveStore {
    inner: InMemoryAgentSessionRegistryStore,
    fail_saves: Mutex<bool>,
}

impl FailingSaveStore {
    fn fail_saves(&self) {
        *self.fail_saves.lock().expect("test store lock poisoned") = true;
    }
}

impl AgentSessionRegistryStore for FailingSaveStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        self.inner.insert(record)
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if *self.fail_saves.lock().expect("test store lock poisoned") {
                return Err(InteractionError::internal("injected snapshot save failure"));
            }
            self.inner.save(record).await
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove(session_id)
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        self.inner.get(session_id)
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        self.inner.list()
    }
}

struct ManyDeltaModel {
    deltas: usize,
}

impl ModelProvider for ManyDeltaModel {
    fn id(&self) -> &str {
        "many-delta"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let mut events = Vec::with_capacity(self.deltas + 2);
            events.push(ModelStreamEvent::Started {
                stream_id: "many-delta-stream".into(),
            });
            for _ in 0..self.deltas {
                events.push(ModelStreamEvent::TextDelta { text: "x".into() });
            }
            events.push(ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            });
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
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
