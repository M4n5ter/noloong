#![cfg(feature = "registry-store-object")]

use noloong_agent::{
    AgentManifest, AgentSession, AutomationRecord, AutomationStatus, AutomationTarget,
    AutomationTimeSchedule, AutomationTrigger, GoalRecord,
    interaction::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentRuntimeProfile, AgentSessionRecord,
        AgentSessionRegistry, AgentSessionRegistryStore, InteractionError, InteractionFuture,
        InteractionProfileDescriptor, InteractionSessionStatus, OpenDalAgentSessionRegistryStore,
        OpenDalAgentSessionRegistryStoreConfig,
    },
};
use noloong_agent_core::{
    AgentMessage, AgentRuntime, AgentState, BoxFuture, CancellationToken, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, RunStatus, StopReason,
};
use opendal::{Operator, services::Memory};
use serde_json::Map;
use std::sync::Arc;

#[tokio::test]
async fn object_store_insert_get_list_save_remove() {
    let store = object_store("sessions");
    store.insert(record("root")).await.unwrap();
    assert!(
        store
            .insert(record("root"))
            .await
            .expect_err("duplicate insert fails")
            .message
            .contains("session already exists")
    );

    let mut stored = store.get("root").await.unwrap().unwrap();
    stored.role = Some("updated".into());
    store.save(stored).await.unwrap();

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].role.as_deref(), Some("updated"));

    store.remove("root").await.unwrap();
    assert!(store.get("root").await.unwrap().is_none());
}

#[tokio::test]
async fn object_store_key_encoding_is_path_safe() {
    let store = object_store("encoded");
    let session_id = "root/with space/中文";
    store.insert(record(session_id)).await.unwrap();

    let listed = store.list().await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, session_id);
}

#[tokio::test]
async fn object_store_persists_goal_and_automation_records() {
    let store = object_store("goal-automation");
    let goal = GoalRecord::new("root/with space", "finish object store support");
    store.save_goal(goal.clone()).await.unwrap();
    assert_eq!(
        store.get_goal(&goal.session_id).await.unwrap(),
        Some(goal.clone())
    );
    assert_eq!(store.list_goals().await.unwrap(), vec![goal.clone()]);
    store.remove_goal(&goal.session_id).await.unwrap();
    assert!(store.get_goal(&goal.session_id).await.unwrap().is_none());

    let automation = automation_record("automation/object");
    store.insert_automation(automation.clone()).await.unwrap();
    assert!(
        store
            .insert_automation(automation.clone())
            .await
            .expect_err("duplicate automation insert fails")
            .message
            .contains("automation already exists")
    );
    assert_eq!(
        store
            .get_automation(&automation.automation_id)
            .await
            .unwrap(),
        Some(automation.clone())
    );
    assert_eq!(
        store.list_automations().await.unwrap(),
        vec![automation.clone()]
    );
    store
        .remove_automation(&automation.automation_id)
        .await
        .unwrap();
    assert!(
        store
            .get_automation(&automation.automation_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn object_store_scans_and_updates_automation_schedule_index() {
    let store = object_store("automation-schedule");
    let due = automation_record_with_schedule("due", AutomationStatus::Active, Some(90));
    let future = automation_record_with_schedule("future", AutomationStatus::Active, Some(150));
    let paused = automation_record_with_schedule("paused", AutomationStatus::Paused, Some(80));
    store.insert_automation(due.clone()).await.unwrap();
    store.insert_automation(future.clone()).await.unwrap();
    store.insert_automation(paused).await.unwrap();

    let scan = store.scan_automation_schedule(100).await.unwrap();
    assert_eq!(scan.due_automation_ids, vec!["due"]);
    assert_eq!(scan.next_fire_at_ms, Some(150));

    let mut due_later = due;
    due_later.next_fire_at_ms = Some(175);
    store.save_automation(due_later).await.unwrap();
    store
        .remove_automation(&future.automation_id)
        .await
        .unwrap();

    let scan = store.scan_automation_schedule(100).await.unwrap();
    assert!(scan.due_automation_ids.is_empty());
    assert_eq!(scan.next_fire_at_ms, Some(175));
}

#[tokio::test]
async fn object_store_prefix_isolation() {
    let operator = memory_operator();
    let first = OpenDalAgentSessionRegistryStore::new(
        operator.clone(),
        OpenDalAgentSessionRegistryStoreConfig::new("first"),
    );
    let second = OpenDalAgentSessionRegistryStore::new(
        operator,
        OpenDalAgentSessionRegistryStoreConfig::new("second"),
    );

    first.insert(record("root")).await.unwrap();

    assert_eq!(first.list().await.unwrap().len(), 1);
    assert!(second.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn object_store_registry_recovers_session_across_instances() {
    let operator = memory_operator();
    let first_store = Arc::new(OpenDalAgentSessionRegistryStore::new(
        operator.clone(),
        OpenDalAgentSessionRegistryStoreConfig::new("registry"),
    ));
    first_store.insert(record("root")).await.unwrap();
    let first_registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        first_store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let descriptor = first_registry
        .get_descriptor("root")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(descriptor.status, InteractionSessionStatus::Completed);

    let second_store = Arc::new(OpenDalAgentSessionRegistryStore::new(
        operator,
        OpenDalAgentSessionRegistryStoreConfig::new("registry"),
    ));
    let second_registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        second_store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let live = second_registry.get("root").await.unwrap().unwrap();
    live.agent().prompt("hello").await.unwrap();

    assert_eq!(
        second_registry
            .get_descriptor("root")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionSessionStatus::Completed
    );
}

#[tokio::test]
async fn object_store_running_restore_is_written_back() {
    let store = object_store("running");
    let mut running = record("root");
    running.state.status = RunStatus::Running;
    running.state.active_phase = Some("model_stream".into());
    store.insert(running).await.unwrap();
    let registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        Arc::new(store.clone()) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();

    let descriptor = registry.get_descriptor("root").await.unwrap().unwrap();

    assert_eq!(descriptor.status, InteractionSessionStatus::Failed);
    assert_eq!(
        store.get("root").await.unwrap().unwrap().state.status,
        RunStatus::Failed
    );
}

fn object_store(prefix: &str) -> OpenDalAgentSessionRegistryStore {
    OpenDalAgentSessionRegistryStore::new(
        memory_operator(),
        OpenDalAgentSessionRegistryStoreConfig::new(prefix),
    )
}

fn memory_operator() -> Operator {
    Operator::new(Memory::default()).unwrap().finish()
}

fn record(session_id: &str) -> AgentSessionRecord {
    AgentSessionRecord {
        schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION,
        session_id: session_id.into(),
        profile_id: "default".into(),
        parent_session_id: None,
        role: None,
        manifest: AgentManifest::default(),
        state: AgentState {
            run_id: Some("run-1".into()),
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

fn automation_record(automation_id: &str) -> AutomationRecord {
    automation_record_with_schedule(automation_id, AutomationStatus::Active, Some(123))
}

fn automation_record_with_schedule(
    automation_id: &str,
    status: AutomationStatus,
    next_fire_at_ms: Option<u64>,
) -> AutomationRecord {
    let mut automation = AutomationRecord::new(
        automation_id,
        AutomationTarget::ExistingSession {
            session_id: "root".into(),
        },
        AutomationTrigger::Time {
            schedule: AutomationTimeSchedule::Once { at_ms: 123 },
        },
        AgentMessage::user("automation-prompt", "hello"),
    );
    automation.status = status;
    automation.next_fire_at_ms = next_fire_at_ms;
    automation
}

fn text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: InteractionProfileDescriptor {
            profile_id: profile_id.into(),
            display_name: profile_id.into(),
            description: None,
            default_manifest_patches: Vec::new(),
            metadata: Map::new(),
        },
    })
}

struct TestProfile {
    descriptor: InteractionProfileDescriptor,
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
            AgentRuntime::builder()
                .with_model_provider(Arc::new(TextModel))
                .build()
                .map_err(InteractionError::from)
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
