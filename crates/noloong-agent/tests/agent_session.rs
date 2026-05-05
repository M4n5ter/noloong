use noloong_agent::{
    AgentManifest, AgentSession, ApprovalPolicy, BackgroundCompletionConfig, BuiltInToolName,
    Locale, ManifestPatch, StartCommandRequest,
    approval::{
        allow_decision as approval_allow_decision, deny_decision as approval_deny_decision,
    },
};
use noloong_agent_core::{
    Agent, AgentEventKind, AgentMessage, BoxFuture, CancellationToken, ContentBlock, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, RunStatus, StopReason, ToolApprovalRequest,
    ToolApprovalRequestSpec, ToolApprovalResolution, ToolCall,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    sync::Notify,
    time::{Duration, timeout},
};

#[test]
fn agent_session_tool_patch_takes_effect_next_turn() {
    let manifest = AgentManifest::default();
    let session = AgentSession::builder().with_manifest(manifest).build();
    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .build()
        .unwrap();

    assert!(runtime.tool("host.exec.start").is_err());

    let proposal = session
        .proposal_store()
        .record_pending_proposal(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();
    let applied = session.apply_approved_manifest_patches().unwrap();
    assert!(applied.is_empty());

    session
        .proposal_store()
        .approve_proposal(&proposal.proposal_id)
        .unwrap();
    let applied = session.apply_approved_manifest_patches().unwrap();
    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .build()
        .unwrap();

    assert_eq!(applied, vec!["manifest-proposal-1".to_string()]);
    assert!(runtime.tool("host.exec.start").is_ok());
}

#[tokio::test]
async fn agent_session_rebuild_preserves_background_jobs() {
    let manifest = AgentManifest::default().with_enabled_tool(BuiltInToolName::HostExecStart);
    let session = AgentSession::builder().with_manifest(manifest).build();
    let manager = session.process_manager();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "sleep 1".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(10),
        })
        .await
        .unwrap();

    let proposal = session
        .proposal_store()
        .record_pending_proposal(ManifestPatch::UpdateApprovalPolicy {
            policy: ApprovalPolicy::AllowAll,
        })
        .unwrap();
    session
        .proposal_store()
        .approve_proposal(&proposal.proposal_id)
        .unwrap();
    session.apply_approved_manifest_patches().unwrap();
    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .build()
        .unwrap();
    let jobs = session.process_manager().list().await.unwrap();

    assert!(runtime.tool("host.exec.start").is_ok());
    assert!(jobs.iter().any(|job| job.job_id == snapshot.job_id));
    session.process_manager().close().await.unwrap();
}

#[tokio::test]
async fn agent_session_records_approved_built_in_tool_call_for_current_session() {
    let session = approval_cache_session();
    seed_approval_cache(&session, "printf cached").await;

    let cached_agent = host_exec_agent(&session, "printf cached");
    cached_agent.prompt("cached command").await.unwrap();

    let state = cached_agent.state().await;
    assert!(matches!(state.status, RunStatus::Completed));
    assert!(state.pending_tool_approvals.is_empty());
    session.process_manager().close().await.unwrap();
}

#[tokio::test]
async fn agent_session_approval_cache_does_not_cover_changed_commands() {
    let session = approval_cache_session();
    seed_approval_cache(&session, "printf cached").await;

    let changed_agent = host_exec_agent(&session, "printf changed");
    changed_agent.prompt("changed command").await.unwrap();

    let state = changed_agent.state().await;
    assert!(matches!(state.status, RunStatus::Paused));
    assert_eq!(state.pending_tool_approvals.len(), 1);
    session.process_manager().close().await.unwrap();
}

#[test]
fn agent_session_approval_cache_ignores_denials_and_external_hooks() {
    let session = approval_cache_session();
    let tool_call = host_exec_start_tool_call("printf cached");
    let approval = tool_approval_request(
        tool_call.clone(),
        Some("noloong.builtin.approval"),
        serde_json::json!({"approvalCacheKey": "cache-key-test"}),
    );
    let external = tool_approval_request(
        tool_call.clone(),
        Some("external.hook"),
        serde_json::json!({"approvalCacheKey": "cache-key-test"}),
    );
    let missing_metadata = tool_approval_request(
        tool_call,
        Some("noloong.builtin.approval"),
        serde_json::json!({}),
    );

    assert!(!session.record_tool_approval_resolution(&approval, &test_deny_decision()));
    assert!(!session.record_tool_approval_resolution(&external, &test_allow_decision()));
    assert!(!session.record_tool_approval_resolution(&missing_metadata, &test_allow_decision()));
}

#[tokio::test]
async fn agent_session_built_in_tool_audit_includes_permission_metadata() {
    let session = approval_cache_session();
    let events = approve_host_exec_start_with_captured_events(&session, "printf audit").await;

    assert!(events.iter().any(|event| matches!(
        event,
        AgentEventKind::ToolPermissionRequested { permissions, .. }
            if permissions.iter().any(|permission|
                permission.metadata["builtIn"] == true
                    && permission.metadata["capability"] == "host.command"
            )
    )));
    session.process_manager().close().await.unwrap();
}

#[tokio::test]
async fn background_completion_is_queued_until_next_prompt() {
    let session = AgentSession::builder().build();
    let model = Arc::new(CapturingModelProvider::default());
    let agent = Agent::builder()
        .with_runtime(Arc::new(
            session
                .runtime_builder()
                .with_model_provider(model.clone())
                .build()
                .unwrap(),
        ))
        .build()
        .unwrap();
    let _completion_steering = session
        .attach_background_completion_steering(&agent, BackgroundCompletionConfig::default());

    let snapshot = session
        .process_manager()
        .start(StartCommandRequest {
            command: "printf queued".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(1000),
        })
        .await
        .unwrap();
    wait_for_completion_queued(&agent, &snapshot.job_id).await;

    assert_eq!(model.requests_len(), 0);

    agent.prompt("inspect completion").await.unwrap();
    let requests = model.requests();
    let messages = &requests.first().expect("first request exists").messages;
    let completion_index = message_index(
        messages,
        &format!("host-exec-completed-{}", snapshot.job_id),
    );
    let prompt_index = message_index(messages, "user-run-1-1");
    let completion_text = message_text(&messages[completion_index]);

    assert!(completion_index < prompt_index);
    assert!(completion_text.contains("Background host command completed."));
    assert!(completion_text.contains("queued"));
}

#[tokio::test]
async fn background_completion_uses_manifest_locale() {
    let manifest = AgentManifest {
        locale: Locale::Zh,
        ..Default::default()
    };
    let session = AgentSession::builder().with_manifest(manifest).build();
    let model = Arc::new(CapturingModelProvider::default());
    let agent = Agent::builder()
        .with_runtime(Arc::new(
            session
                .runtime_builder()
                .with_model_provider(model.clone())
                .build()
                .unwrap(),
        ))
        .build()
        .unwrap();
    let _completion_steering = session
        .attach_background_completion_steering(&agent, BackgroundCompletionConfig::default());

    let snapshot = session
        .process_manager()
        .start(StartCommandRequest {
            command: "printf locale".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(1000),
        })
        .await
        .unwrap();
    wait_for_completion_queued(&agent, &snapshot.job_id).await;

    agent.prompt("inspect completion").await.unwrap();
    let requests = model.requests();
    let messages = &requests.first().expect("first request exists").messages;
    let completion = messages
        .iter()
        .find(|message| message.id.starts_with("host-exec-completed-"))
        .expect("completion message exists");
    let completion_text = message_text(completion);

    assert!(completion_text.contains("后台宿主机命令已完成"));
    assert!(!completion_text.contains("Background host command completed"));
}

#[tokio::test]
async fn background_completion_during_active_run_uses_steering_boundary() {
    let session = AgentSession::builder().build();
    let model = Arc::new(BlockingCaptureModel::default());
    let agent = Agent::builder()
        .with_runtime(Arc::new(
            session
                .runtime_builder()
                .with_model_provider(model.clone())
                .build()
                .unwrap(),
        ))
        .build()
        .unwrap();
    let _completion_steering = session
        .attach_background_completion_steering(&agent, BackgroundCompletionConfig::default());
    let running_agent = agent.clone();
    let handle = tokio::spawn(async move { running_agent.prompt("start active run").await });

    model.wait_for_first_request().await;
    let snapshot = session
        .process_manager()
        .start(StartCommandRequest {
            command: "printf active".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(1000),
        })
        .await
        .unwrap();
    wait_for_completion_queued(&agent, &snapshot.job_id).await;
    model.release_first_request();
    handle.await.expect("prompt task joins").unwrap();

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    let second_messages = &requests[1].messages;
    let completion_index = message_index(
        second_messages,
        &format!("host-exec-completed-{}", snapshot.job_id),
    );
    assert!(message_text(&second_messages[completion_index]).contains("active"));
}

async fn wait_for_completion_queued(agent: &Agent, job_id: &str) {
    let message_id = format!("host-exec-completed-{job_id}");
    timeout(Duration::from_secs(1), async {
        loop {
            if agent
                .queued_steering_messages()
                .iter()
                .any(|message| message.message.id == message_id)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completion steering message is queued");
}

fn approval_cache_session() -> AgentSession {
    let manifest = AgentManifest::default().with_enabled_tool(BuiltInToolName::HostExecStart);
    AgentSession::builder().with_manifest(manifest).build()
}

fn host_exec_agent(session: &AgentSession, command: &str) -> Agent {
    Agent::builder()
        .with_runtime(Arc::new(
            session
                .runtime_builder()
                .with_model_provider(Arc::new(HostExecCommandModel::new(command)))
                .build()
                .unwrap(),
        ))
        .build()
        .unwrap()
}

async fn seed_approval_cache(session: &AgentSession, command: &str) {
    approve_host_exec_start(session, command, None).await;
}

async fn approve_host_exec_start_with_captured_events(
    session: &AgentSession,
    command: &str,
) -> Vec<AgentEventKind> {
    let events = Arc::new(Mutex::new(Vec::new()));
    approve_host_exec_start(session, command, Some(Arc::clone(&events))).await;
    events
        .lock()
        .expect("captured events lock poisoned")
        .clone()
}

async fn approve_host_exec_start(
    session: &AgentSession,
    command: &str,
    events: Option<Arc<Mutex<Vec<AgentEventKind>>>>,
) {
    let agent = host_exec_agent(session, command);
    if let Some(events) = &events {
        let captured_events = Arc::clone(events);
        agent.subscribe(move |event| {
            let captured_events = Arc::clone(&captured_events);
            async move {
                captured_events
                    .lock()
                    .expect("captured events lock poisoned")
                    .push(event.kind);
                Ok(())
            }
        });
    }
    agent.prompt("approval cache seed").await.unwrap();

    let pending = agent.pending_tool_approvals().await;
    assert_eq!(pending.len(), 1);
    let (approval_id, approval) = pending.iter().next().expect("approval exists");
    assert_eq!(
        approval.tool_call.name,
        BuiltInToolName::HostExecStart.as_str()
    );
    assert_eq!(
        approval.request.metadata["classificationDecision"],
        "needs_approval"
    );
    assert!(approval.request.metadata.get("approvalCacheKey").is_some());

    let decision = test_allow_decision();
    assert!(session.record_tool_approval_resolution(approval, &decision));
    agent
        .resume_tool_approval(ToolApprovalResolution {
            approval_id: approval_id.clone(),
            decision,
        })
        .await
        .unwrap();
    assert!(matches!(agent.state().await.status, RunStatus::Completed));
    agent.wait_for_idle().await;
}

fn host_exec_start_tool_call(command: &str) -> ToolCall {
    ToolCall {
        id: "host-exec-start-test".into(),
        name: BuiltInToolName::HostExecStart.as_str().into(),
        arguments: host_exec_start_arguments(command),
    }
}

fn host_exec_start_arguments(command: &str) -> serde_json::Value {
    serde_json::json!({
        "command": command,
        "shell": "sh",
        "cwd": ".",
        "pipeStdin": false,
        "foregroundWaitMs": 1000
    })
}

fn tool_approval_request(
    tool_call: ToolCall,
    hook_id: Option<&str>,
    metadata: serde_json::Value,
) -> ToolApprovalRequest {
    ToolApprovalRequest {
        approval_id: "approval-test".into(),
        tool_call,
        permissions: Vec::new(),
        hook_id: hook_id.map(str::to_owned),
        request: ToolApprovalRequestSpec {
            prompt: None,
            reason: None,
            expires_at_ms: None,
            metadata,
        },
    }
}

fn test_allow_decision() -> noloong_agent_core::ToolPermissionDecision {
    approval_allow_decision("approved by test", "test", serde_json::json!({}))
}

fn test_deny_decision() -> noloong_agent_core::ToolPermissionDecision {
    approval_deny_decision("denied by test", "test", serde_json::json!({}))
}

struct DummyModelProvider;

impl ModelProvider for DummyModelProvider {
    fn id(&self) -> &str {
        "dummy"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        _stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async {
            Ok(vec![ModelStreamEvent::Finished {
                stop_reason: noloong_agent_core::StopReason::Stop,
            }])
        })
    }
}

struct HostExecCommandModel {
    command: String,
    calls: AtomicU64,
}

impl HostExecCommandModel {
    fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            calls: AtomicU64::new(0),
        }
    }
}

impl ModelProvider for HostExecCommandModel {
    fn id(&self) -> &str {
        "host-exec-command"
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
                        stream_id: "host-exec-command-1".into(),
                    },
                    ModelStreamEvent::ToolCall {
                        tool_call: host_exec_start_tool_call(&self.command),
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::ToolUse,
                    },
                ]
            } else {
                vec![
                    ModelStreamEvent::Started {
                        stream_id: "host-exec-command-2".into(),
                    },
                    ModelStreamEvent::TextDelta {
                        text: "command complete".into(),
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

#[derive(Default)]
struct CapturingModelProvider {
    requests: Mutex<Vec<ModelRequest>>,
}

impl CapturingModelProvider {
    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
            .lock()
            .expect("captured requests lock poisoned")
            .clone()
    }

    fn requests_len(&self) -> usize {
        self.requests
            .lock()
            .expect("captured requests lock poisoned")
            .len()
    }
}

impl ModelProvider for CapturingModelProvider {
    fn id(&self) -> &str {
        "capturing"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        _stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            self.requests
                .lock()
                .expect("captured requests lock poisoned")
                .push(request);
            Ok(vec![ModelStreamEvent::Finished {
                stop_reason: noloong_agent_core::StopReason::Stop,
            }])
        })
    }
}

#[derive(Default)]
struct BlockingCaptureModel {
    calls: AtomicU64,
    requests: Mutex<Vec<ModelRequest>>,
    first_request_seen: Notify,
    release_first_request: Notify,
}

impl BlockingCaptureModel {
    async fn wait_for_first_request(&self) {
        loop {
            if self.requests_len() > 0 {
                return;
            }
            self.first_request_seen.notified().await;
        }
    }

    fn release_first_request(&self) {
        self.release_first_request.notify_waiters();
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
            .lock()
            .expect("captured requests lock poisoned")
            .clone()
    }

    fn requests_len(&self) -> usize {
        self.requests
            .lock()
            .expect("captured requests lock poisoned")
            .len()
    }
}

impl ModelProvider for BlockingCaptureModel {
    fn id(&self) -> &str {
        "blocking-capture"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        _stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests
                .lock()
                .expect("captured requests lock poisoned")
                .push(request);
            if call == 0 {
                self.first_request_seen.notify_waiters();
                self.release_first_request.notified().await;
            }
            Ok(vec![ModelStreamEvent::Finished {
                stop_reason: noloong_agent_core::StopReason::Stop,
            }])
        })
    }
}

fn message_index(messages: &[AgentMessage], id: &str) -> usize {
    messages
        .iter()
        .position(|message| message.id == id)
        .expect("message exists")
}

fn message_text(message: &AgentMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
