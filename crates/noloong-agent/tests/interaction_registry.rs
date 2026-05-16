use noloong_agent::{
    AUTOMATION_SESSION_METADATA_KEY, AUTOMATION_SYSTEM_PROMPT_ADDITION_ID, AgentManifest,
    AgentSession, AgentSessionRegistryOptions, AgentSystemPrompt, AutomationCreateRequest,
    AutomationPromptInput, AutomationRecord, AutomationTarget, AutomationTimeSchedule,
    AutomationTrigger, BuiltInToolName, GOAL_AUDIT_SOURCE_TYPE, GoalRecord, GoalSetRequest,
    ManifestPatch, SystemPromptAddition,
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
    ContentBlock, EventStore, InMemoryEventStore, ModelProvider, ModelRequest, ModelStreamEvent,
    ModelStreamSink, QueueMode, RunStatus, StopReason, ToolApprovalRequest,
    ToolApprovalRequestSpec, ToolApprovalResolution, ToolCall, ToolPermissionDecision,
    ToolPermissionOutcome, ToolRequest,
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
            system_prompt: AgentSystemPrompt::custom("Persisted prompt"),
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
        AgentSystemPrompt::custom("Use the runtime profile prompt.")
    );
    assert!(
        created
            .manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
}

#[tokio::test]
async fn interaction_registry_applies_request_manifest_patches_after_profile_defaults() {
    let mut descriptor = descriptor("patched");
    descriptor.default_manifest_patches = vec![ManifestPatch::ReplaceSystemPrompt {
        prompt: "Use the runtime profile prompt.".into(),
    }];
    let registry = AgentSessionRegistry::new(Arc::new(TestProfile {
        descriptor,
        model: Arc::new(TextModel),
        build_delay_ms: 0,
        build_count: None,
    }))
    .unwrap();

    let created = registry
        .create_session(AgentSessionCreateRequest {
            manifest_patches: vec![ManifestPatch::UpsertSystemPromptAddition {
                addition: SystemPromptAddition::new(
                    "interaction.telegram",
                    "Current interaction channel: Telegram.",
                ),
            }],
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    assert_eq!(
        created.manifest.system_prompt,
        AgentSystemPrompt::Custom {
            prompt: "Use the runtime profile prompt.".into(),
            additions: vec![SystemPromptAddition::new(
                "interaction.telegram",
                "Current interaction channel: Telegram."
            )],
        }
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
    record.manifest.system_prompt = AgentSystemPrompt::custom("Persisted prompt");
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
        restored.session().manifest().effective_system_prompt(),
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
async fn interaction_registry_mounts_subagent_tools_for_root_only() {
    let model = Arc::new(CapturingToolRequestModel::default());
    let registry = AgentSessionRegistry::new(runtime_tool_profile("default", model.clone()))
        .expect("registry should build");
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    registry
        .get("parent")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("root")
        .await
        .unwrap();
    registry
        .spawn_subagent(SubagentSpawnRequest {
            parent_session_id: "parent".into(),
            initial_prompt: Some(AgentMessage::user("child-task", "child")),
            ..SubagentSpawnRequest::default()
        })
        .await
        .unwrap();

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    assert_tool_names_include(
        &requests[0],
        &[
            BuiltInToolName::SubagentSpawn.as_str(),
            BuiltInToolName::SubagentWait.as_str(),
            BuiltInToolName::SubagentResult.as_str(),
            BuiltInToolName::SubagentList.as_str(),
        ],
    );
    assert_tool_names_exclude(
        &requests[1],
        &[
            BuiltInToolName::SubagentSpawn.as_str(),
            BuiltInToolName::SubagentWait.as_str(),
            BuiltInToolName::SubagentResult.as_str(),
            BuiltInToolName::SubagentList.as_str(),
        ],
    );
}

#[tokio::test]
async fn interaction_registry_subagent_tools_spawn_wait_and_read_final_output() {
    let registry = AgentSessionRegistry::new(runtime_tool_profile(
        "default",
        Arc::new(SubagentWorkflowModel::default()),
    ))
    .unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    registry
        .get("parent")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("delegate")
        .await
        .unwrap();

    let child = registry
        .get_descriptor("session-1")
        .await
        .unwrap()
        .expect("child should exist");
    assert_eq!(child.parent_session_id.as_deref(), Some("parent"));
    assert_eq!(child.role.as_deref(), Some("reviewer"));
    assert_eq!(child.status, InteractionSessionStatus::Completed);
    assert_eq!(
        child.state.messages.last().unwrap().content[0],
        noloong_agent_core::ContentBlock::Text {
            text: "child final text".into()
        }
    );

    let parent = registry
        .get_descriptor("parent")
        .await
        .unwrap()
        .expect("parent should exist");
    assert_eq!(parent.status, InteractionSessionStatus::Completed);
    assert_eq!(
        parent.state.messages.last().unwrap().content[0],
        noloong_agent_core::ContentBlock::Text {
            text: "parent done".into()
        }
    );
}

#[tokio::test]
async fn interaction_registry_subagent_spawn_drops_transient_prompt_additions() {
    let registry = AgentSessionRegistry::new(runtime_tool_profile(
        "default",
        Arc::new(SubagentWorkflowModel::default()),
    ))
    .unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent".into()),
            manifest_patches: vec![
                ManifestPatch::UpsertSystemPromptAddition {
                    addition: SystemPromptAddition::new("transient.smoke", "parent-only"),
                },
                ManifestPatch::UpsertSystemPromptAddition {
                    addition: SystemPromptAddition::new("stable.context", "child-visible"),
                },
            ],
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    registry
        .get("parent")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("delegate")
        .await
        .unwrap();

    let child = registry
        .get_descriptor("session-1")
        .await
        .unwrap()
        .expect("child should exist");
    let addition_ids = child
        .manifest
        .system_prompt
        .additions()
        .iter()
        .map(|addition| addition.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(addition_ids, vec!["stable.context"]);
}

#[tokio::test]
async fn interaction_registry_subagent_result_rejects_non_child_sessions() {
    let registry = AgentSessionRegistry::new(runtime_tool_profile("default", Arc::new(TextModel)))
        .expect("registry should build");
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent-a".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("parent-b".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let child_b = registry
        .spawn_subagent(SubagentSpawnRequest {
            parent_session_id: "parent-b".into(),
            ..SubagentSpawnRequest::default()
        })
        .await
        .unwrap();
    let parent_a = registry
        .get("parent-a")
        .await
        .unwrap()
        .expect("parent-a should exist");
    let runtime = parent_a
        .session()
        .runtime_builder()
        .with_model_provider(Arc::new(TextModel))
        .build()
        .unwrap();
    let result_tool = runtime
        .tool(BuiltInToolName::SubagentResult.as_str())
        .expect("root session should mount subagent result tool");

    let error = result_tool
        .execute_tool(
            ToolRequest {
                run_id: "run-test".into(),
                turn_id: 1,
                tool_call_id: "tool-call-test".into(),
                tool_name: BuiltInToolName::SubagentResult.as_str().into(),
                arguments: json!({"sessionId": child_b.session_id}),
                state: AgentState::default(),
            },
            CancellationToken::new(),
        )
        .await
        .expect_err("cross-parent access should fail");

    assert!(error.to_string().contains("direct child of `parent-a`"));
}

#[tokio::test]
async fn interaction_registry_goal_audit_runs_after_turn_and_mounts_tool() {
    let model = Arc::new(CapturingToolRequestModel::default());
    let registry = AgentSessionRegistry::new(runtime_tool_profile("default", model.clone()))
        .expect("registry should build");
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("goal-session".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .set_goal(GoalSetRequest {
            session_id: "goal-session".into(),
            objective: "finish goal audit".into(),
            token_budget: None,
            metadata: Map::new(),
        })
        .await
        .unwrap();

    registry
        .get("goal-session")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("work")
        .await
        .unwrap();

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    assert_tool_names_include(&requests[0], &[BuiltInToolName::GoalUpdate.as_str()]);
    assert!(requests[1].messages.iter().any(|message| {
        message
            .metadata
            .get("source")
            .and_then(|source| source.get("type"))
            .is_some_and(|source_type| source_type == GOAL_AUDIT_SOURCE_TYPE)
    }));
    let goal = registry.get_goal("goal-session").await.unwrap().unwrap();
    assert!(!goal.last_audit.unwrap().pending);
}

#[tokio::test]
async fn interaction_registry_goal_tool_not_mounted_without_goal() {
    let model = Arc::new(CapturingToolRequestModel::default());
    let registry = AgentSessionRegistry::new(runtime_tool_profile("default", model.clone()))
        .expect("registry should build");
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("plain-session".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();

    registry
        .get("plain-session")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .prompt("work")
        .await
        .unwrap();

    assert_tool_names_exclude(
        &model.requests()[0],
        &[BuiltInToolName::GoalUpdate.as_str()],
    );
}

#[tokio::test]
async fn interaction_registry_fires_automation_to_existing_session() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("automation-target".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-existing".into()),
            target: AutomationTarget::ExistingSession {
                session_id: "automation-target".into(),
            },
            trigger: once_trigger(current_unix_ms_for_test() + 60_000),
            prompt: AutomationPromptInput::Text {
                text: "automation work".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();

    let automation = registry
        .fire_automation("automation-existing")
        .await
        .unwrap();
    let target = registry
        .get_descriptor("automation-target")
        .await
        .unwrap()
        .unwrap();

    assert!(automation.last_fired_at_ms.is_some());
    assert_eq!(target.status, InteractionSessionStatus::Completed);
    assert_eq!(
        target.state.messages[0].metadata["source"]["type"],
        "automation"
    );
    assert!(matches!(
        &target.state.messages[0].content[0],
        ContentBlock::Text { text } if text.contains("Automation `automation-existing` fired")
    ));
}

#[tokio::test]
async fn interaction_registry_fires_automation_to_busy_session_as_steering() {
    let registry = AgentSessionRegistry::new(blocking_profile("blocking")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("busy-target".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let registered = registry.get("busy-target").await.unwrap().unwrap();
    let agent = registered.agent().clone();
    let run = tokio::spawn(async move { agent.prompt("block").await });
    wait_until_status(&registry, "busy-target", InteractionSessionStatus::Running).await;
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-busy".into()),
            target: AutomationTarget::ExistingSession {
                session_id: "busy-target".into(),
            },
            trigger: once_trigger(current_unix_ms_for_test() + 60_000),
            prompt: AutomationPromptInput::Text {
                text: "automation observation".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();

    registry.fire_automation("automation-busy").await.unwrap();
    let queued = registry
        .get("busy-target")
        .await
        .unwrap()
        .unwrap()
        .agent()
        .queued_steering_messages();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].message.metadata["source"]["type"], "automation");

    registry
        .delete_session(
            "busy-target",
            AgentSessionDeleteOptions { force_abort: true },
        )
        .await
        .unwrap();
    let _ = run.await.unwrap();
}

#[tokio::test]
async fn interaction_registry_creates_pure_automation_session() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-new-session".into()),
            target: AutomationTarget::NewSession {
                session_id: None,
                profile_id: None,
            },
            trigger: once_trigger(current_unix_ms_for_test() + 60_000),
            prompt: AutomationPromptInput::Text {
                text: "automation work".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();

    let automation = registry
        .fire_automation("automation-new-session")
        .await
        .unwrap();
    let session_id = automation
        .target
        .session_id()
        .expect("automation target should be materialized")
        .to_owned();
    let descriptor = registry
        .get_descriptor(&session_id)
        .await
        .unwrap()
        .expect("automation session should exist");

    assert_eq!(
        descriptor.metadata[AUTOMATION_SESSION_METADATA_KEY]["automationId"],
        "automation-new-session"
    );
    assert!(
        descriptor
            .manifest
            .system_prompt
            .additions()
            .iter()
            .any(|addition| addition.id == AUTOMATION_SYSTEM_PROMPT_ADDITION_ID)
    );
}

#[tokio::test]
async fn interaction_registry_time_runner_fires_due_automation() {
    let registry = AgentSessionRegistry::new(text_profile("default")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("runner-target".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-runner".into()),
            target: AutomationTarget::ExistingSession {
                session_id: "runner-target".into(),
            },
            trigger: once_trigger(current_unix_ms_for_test() + 20),
            prompt: AutomationPromptInput::Text {
                text: "runner work".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();

    for _ in 0..100 {
        let automation = registry
            .get_automation("automation-runner")
            .await
            .unwrap()
            .unwrap();
        if automation.last_fired_at_ms.is_some() {
            assert_eq!(
                automation.status,
                noloong_agent::AutomationStatus::Completed
            );
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("automation runner did not fire due automation");
}

#[tokio::test]
async fn interaction_registry_can_disable_automation_runner() {
    let registry = AgentSessionRegistry::with_store_and_options(
        "default",
        vec![text_profile("default")],
        Arc::new(InMemoryAgentSessionRegistryStore::default())
            as Arc<dyn AgentSessionRegistryStore>,
        AgentSessionRegistryOptions {
            automation_runner_enabled: false,
        },
    )
    .unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("runner-disabled-target".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-runner-disabled".into()),
            target: AutomationTarget::ExistingSession {
                session_id: "runner-disabled-target".into(),
            },
            trigger: once_trigger(current_unix_ms_for_test()),
            prompt: AutomationPromptInput::Text {
                text: "runner disabled work".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();

    sleep(Duration::from_millis(50)).await;

    let automation = registry
        .get_automation("automation-runner-disabled")
        .await
        .unwrap()
        .unwrap();
    assert!(automation.last_fired_at_ms.is_none());
}

#[tokio::test]
async fn interaction_registry_rejects_concurrent_automation_fire() {
    let registry = AgentSessionRegistry::new(blocking_profile("blocking")).unwrap();
    registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("double-fire-target".into()),
            ..AgentSessionCreateRequest::default()
        })
        .await
        .unwrap();
    registry
        .create_automation(AutomationCreateRequest {
            automation_id: Some("automation-double-fire".into()),
            target: AutomationTarget::ExistingSession {
                session_id: "double-fire-target".into(),
            },
            trigger: once_trigger(current_unix_ms_for_test() + 60_000),
            prompt: AutomationPromptInput::Text {
                text: "blocking automation".into(),
            },
            metadata: Map::new(),
        })
        .await
        .unwrap();
    let first_registry = registry.clone();
    let first_fire = tokio::spawn(async move {
        first_registry
            .fire_automation("automation-double-fire")
            .await
    });
    wait_until_status(
        &registry,
        "double-fire-target",
        InteractionSessionStatus::Running,
    )
    .await;

    let error = registry
        .fire_automation("automation-double-fire")
        .await
        .expect_err("concurrent fire should be rejected");
    assert_eq!(error.code, INTERACTION_ERROR_BUSY);

    registry
        .delete_session(
            "double-fire-target",
            AgentSessionDeleteOptions { force_abort: true },
        )
        .await
        .unwrap();
    first_fire.await.unwrap().unwrap();
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

fn once_trigger(once_at_ms: u64) -> AutomationTrigger {
    AutomationTrigger::Time {
        schedule: AutomationTimeSchedule {
            once_at_ms: Some(once_at_ms),
            interval_seconds: None,
        },
    }
}

fn current_unix_ms_for_test() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

fn runtime_tool_profile(
    profile_id: &str,
    model: Arc<dyn ModelProvider>,
) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(RuntimeToolProfile {
        descriptor: descriptor(profile_id),
        model,
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

struct RuntimeToolProfile {
    descriptor: InteractionProfileDescriptor,
    model: Arc<dyn ModelProvider>,
}

impl AgentRuntimeProfile for RuntimeToolProfile {
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

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()> {
        self.inner.save_goal(goal)
    }

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>> {
        self.inner.get_goal(session_id)
    }

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>> {
        self.inner.list_goals()
    }

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove_goal(session_id)
    }

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        self.inner.insert_automation(automation)
    }

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        self.inner.save_automation(automation)
    }

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>> {
        self.inner.get_automation(automation_id)
    }

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>> {
        self.inner.list_automations()
    }

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove_automation(automation_id)
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

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()> {
        self.inner.save_goal(goal)
    }

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>> {
        self.inner.get_goal(session_id)
    }

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>> {
        self.inner.list_goals()
    }

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove_goal(session_id)
    }

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        self.inner.insert_automation(automation)
    }

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        self.inner.save_automation(automation)
    }

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>> {
        self.inner.get_automation(automation_id)
    }

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>> {
        self.inner.list_automations()
    }

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()> {
        self.inner.remove_automation(automation_id)
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
struct CapturingToolRequestModel {
    requests: Mutex<Vec<ModelRequest>>,
}

impl CapturingToolRequestModel {
    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
            .lock()
            .expect("captured model requests lock poisoned")
            .clone()
    }
}

impl ModelProvider for CapturingToolRequestModel {
    fn id(&self) -> &str {
        "capturing-tool-request"
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
                .expect("captured model requests lock poisoned")
                .push(request);
            let events = vec![
                ModelStreamEvent::Started {
                    stream_id: "capturing-tool-request-stream".into(),
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
struct SubagentWorkflowModel {
    parent_calls: AtomicU64,
}

impl ModelProvider for SubagentWorkflowModel {
    fn id(&self) -> &str {
        "subagent-workflow"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let can_spawn = request
                .tools
                .iter()
                .any(|tool| tool.name == BuiltInToolName::SubagentSpawn.as_str());
            let events = if !can_spawn {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "subagent-child-stream".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "child final text".into(),
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::Stop,
                    },
                ]
            } else {
                match self.parent_calls.fetch_add(1, Ordering::SeqCst) {
                    0 => vec![
                        ModelStreamEvent::Started {
                            stream_id: "subagent-parent-spawn-stream".into(),
                        },
                        ModelStreamEvent::ToolCall {
                            tool_call: ToolCall {
                                id: "subagent-spawn-test".into(),
                                name: BuiltInToolName::SubagentSpawn.as_str().into(),
                                arguments: json!({
                                    "role": "reviewer",
                                    "prompt": "review this"
                                }),
                            },
                        },
                        ModelStreamEvent::Finished {
                            stop_reason: StopReason::ToolUse,
                        },
                    ],
                    1 => vec![
                        ModelStreamEvent::Started {
                            stream_id: "subagent-parent-wait-stream".into(),
                        },
                        ModelStreamEvent::ToolCall {
                            tool_call: ToolCall {
                                id: "subagent-wait-test".into(),
                                name: BuiltInToolName::SubagentWait.as_str().into(),
                                arguments: json!({
                                    "sessionIds": ["session-1"],
                                    "timeoutMs": 1000
                                }),
                            },
                        },
                        ModelStreamEvent::Finished {
                            stop_reason: StopReason::ToolUse,
                        },
                    ],
                    _ => vec![
                        ModelStreamEvent::Started {
                            stream_id: "subagent-parent-final-stream".into(),
                        },
                        ModelStreamEvent::TextDelta {
                            text: "parent done".into(),
                        },
                        ModelStreamEvent::Finished {
                            stop_reason: StopReason::Stop,
                        },
                    ],
                }
            };
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

fn assert_tool_names_include(request: &ModelRequest, names: &[&str]) {
    for name in names {
        assert!(
            request.tools.iter().any(|tool| tool.name == *name),
            "expected model request to include tool {name}"
        );
    }
}

fn assert_tool_names_exclude(request: &ModelRequest, names: &[&str]) {
    for name in names {
        assert!(
            request.tools.iter().all(|tool| tool.name != *name),
            "expected model request to exclude tool {name}"
        );
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
