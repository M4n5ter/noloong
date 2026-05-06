#![cfg(feature = "registry-store-sqlite")]

use noloong_agent::{
    AgentManifest, AgentSession,
    interaction::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentRuntimeProfile, AgentSessionCreateRequest,
        AgentSessionRecord, AgentSessionRegistry, AgentSessionRegistryStore, InteractionError,
        InteractionFuture, InteractionProfileDescriptor, InteractionSessionStatus,
        SqlAgentSessionRegistryStore, SqlAgentSessionRegistryStoreConfig,
    },
};
use noloong_agent_core::{
    AgentMessage, AgentRuntime, AgentState, BoxFuture, CancellationToken, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, RunStatus, StopReason,
};
use serde_json::{Map, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn sqlite_store_insert_get_list_save_remove() {
    let store = SqlAgentSessionRegistryStore::connect(
        SqlAgentSessionRegistryStoreConfig::sqlite_in_memory(),
    )
    .await
    .unwrap();
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
    stored.metadata.insert("updated".into(), json!(true));
    store.save(stored).await.unwrap();

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].metadata["updated"], true);

    store.remove("root").await.unwrap();
    assert!(store.get("root").await.unwrap().is_none());
}

#[tokio::test]
async fn sqlite_store_recovers_session_across_instances() {
    let db = TempSqliteDb::new("recover");
    let first_store = Arc::new(
        SqlAgentSessionRegistryStore::connect(db.config())
            .await
            .unwrap(),
    );
    let first_registry = AgentSessionRegistry::with_store(
        "default",
        vec![text_profile("default")],
        first_store as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    first_registry
        .create_session(AgentSessionCreateRequest {
            session_id: Some("root".into()),
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
        .prompt("hello")
        .await
        .unwrap();

    let build_count = Arc::new(AtomicU64::new(0));
    let second_store = Arc::new(
        SqlAgentSessionRegistryStore::connect(db.existing_config())
            .await
            .unwrap(),
    );
    let second_registry = AgentSessionRegistry::with_store(
        "default",
        vec![counting_text_profile("default", Arc::clone(&build_count))],
        Arc::clone(&second_store) as Arc<dyn AgentSessionRegistryStore>,
    )
    .unwrap();
    let descriptor = second_registry
        .get_descriptor("root")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(descriptor.status, InteractionSessionStatus::Completed);
    assert_eq!(build_count.load(Ordering::SeqCst), 0);

    let live = second_registry.get("root").await.unwrap().unwrap();
    assert_eq!(build_count.load(Ordering::SeqCst), 1);
    assert_eq!(live.agent().state().await.messages.len(), 2);

    second_registry
        .delete_session("root", Default::default())
        .await
        .unwrap();
    let third_store = SqlAgentSessionRegistryStore::connect(db.existing_config())
        .await
        .unwrap();
    assert!(third_store.get("root").await.unwrap().is_none());
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

fn text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    counting_text_profile(profile_id, Arc::new(AtomicU64::new(0)))
}

fn counting_text_profile(
    profile_id: &str,
    build_count: Arc<AtomicU64>,
) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: InteractionProfileDescriptor {
            profile_id: profile_id.into(),
            display_name: profile_id.into(),
            description: None,
            default_manifest_patches: Vec::new(),
            metadata: Map::new(),
        },
        build_count,
    })
}

struct TestProfile {
    descriptor: InteractionProfileDescriptor,
    build_count: Arc<AtomicU64>,
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
            self.build_count.fetch_add(1, Ordering::SeqCst);
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

struct TempSqliteDb {
    path: PathBuf,
}

impl TempSqliteDb {
    fn new(name: &str) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "noloong-agent-registry-{name}-{}-{id}.sqlite",
                std::process::id()
            )),
        }
    }

    fn config(&self) -> SqlAgentSessionRegistryStoreConfig {
        SqlAgentSessionRegistryStoreConfig::sqlite_file(&self.path)
    }

    fn existing_config(&self) -> SqlAgentSessionRegistryStoreConfig {
        self.config().without_migrations()
    }
}

impl Drop for TempSqliteDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(self.path.with_extension("sqlite-wal"));
    }
}
