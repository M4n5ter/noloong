use noloong_agent::{
    AgentManifest, ApprovalPolicy, BuiltInToolName, Locale, ManifestPatch, ManifestProposalStore,
};

#[test]
fn manifest_patch_applies_prompt_tools_policy() {
    let mut manifest = AgentManifest::default();

    manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt {
            prompt: "New prompt".into(),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::SetLocale { locale: Locale::Zh })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::UpdateApprovalPolicy {
            policy: ApprovalPolicy::AllowAll,
        })
        .unwrap();

    assert_eq!(manifest.system_prompt, "New prompt");
    assert_eq!(manifest.locale, Locale::Zh);
    assert!(
        manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
    assert_eq!(manifest.approval_policy, ApprovalPolicy::AllowAll);
}

#[test]
fn manifest_patch_rejects_invalid_changes() {
    let mut manifest = AgentManifest::default();
    let before = manifest.clone();

    let error = manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt { prompt: " ".into() })
        .unwrap_err();

    assert_eq!(manifest, before);
    assert!(error.to_string().contains("system prompt"));
}

#[test]
fn manifest_patch_rejects_unknown_tool_names() {
    let error = serde_json::from_value::<ManifestPatch>(serde_json::json!({
        "op": "enable_tool",
        "toolName": "host.exec.unknown"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown built-in tool"));
}

#[test]
fn manifest_phase_patch_is_reserved() {
    let mut manifest = AgentManifest::default();

    let error = manifest
        .apply_patch(ManifestPatch::ReservedPhaseProfile {
            description: "replace turn decision".into(),
            metadata: serde_json::json!({}),
        })
        .unwrap_err();

    assert!(error.to_string().contains("reserved"));
}

#[test]
fn manifest_proposal_store_records_without_applying() {
    let store = ManifestProposalStore::default();
    let manifest = AgentManifest::default();

    let proposal = store
        .record_pending_proposal(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();

    assert_eq!(store.pending_len(), 1);
    assert_eq!(store.approved_len(), 0);
    assert_eq!(proposal.summary, "enable tool host.exec.start");
    assert!(
        !manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
}

#[test]
fn manifest_proposal_store_approves_pending_proposals() {
    let store = ManifestProposalStore::default();
    let proposal = store
        .record_pending_proposal(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();

    let approved = store.approve_proposal(&proposal.proposal_id).unwrap();

    assert_eq!(approved.proposal_id, proposal.proposal_id);
    assert_eq!(store.pending_len(), 0);
    assert_eq!(store.approved_len(), 1);
}
