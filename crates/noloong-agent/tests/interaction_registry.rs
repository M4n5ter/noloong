use noloong_agent::{
    AgentManifest, AgentSession, BuiltInToolName, ManifestPatch,
    interaction::{
        AgentRuntimeProfile, AgentSessionCreateRequest, AgentSessionDeleteOptions,
        AgentSessionListFilter, AgentSessionRegistry, INTERACTION_ERROR_BUSY,
        INTERACTION_ERROR_NOT_FOUND, InteractionError, InteractionFuture,
        InteractionProfileDescriptor, InteractionSessionStatus, SubagentSpawnRequest,
    },
};
use noloong_agent_core::{
    AgentCoreError, AgentMessage, AgentRuntime, BoxFuture, CancellationToken, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, StopReason,
};
use serde_json::Map;
use serde_json::json;
use std::sync::Arc;
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

    let listed = registry.list(AgentSessionListFilter::default()).await;
    assert_eq!(listed, vec![created.clone()]);

    let fetched = registry.get_descriptor("root").await.unwrap().unwrap();
    assert_eq!(fetched, created);
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
        registry.list(AgentSessionListFilter::default()).await.len(),
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
        .await;

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
    })
}

fn slow_text_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(TextModel),
        build_delay_ms: 25,
    })
}

fn blocking_profile(profile_id: &str) -> Arc<dyn AgentRuntimeProfile> {
    Arc::new(TestProfile {
        descriptor: descriptor(profile_id),
        model: Arc::new(BlockingModel),
        build_delay_ms: 0,
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
