use noloong_agent::{
    AgentManifest, AgentSession, ApprovalPolicy, ManifestPatch, ProductToolName,
    StartCommandRequest,
};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

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
            tool_name: ProductToolName::HostExecStart,
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
    let manifest = AgentManifest::default().with_enabled_tool(ProductToolName::HostExecStart);
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
