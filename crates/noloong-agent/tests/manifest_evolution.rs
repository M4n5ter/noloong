use noloong_agent::{Catalog, Locale, ManifestPatchProposalTool, ManifestProposalStore};
use noloong_agent_core::{AgentState, CancellationToken, ContentBlock, ToolProvider, ToolRequest};
use serde_json::json;

#[tokio::test]
async fn manifest_proposal_tool_returns_auditable_details() {
    let store = ManifestProposalStore::default();
    let tool = ManifestPatchProposalTool::new(store.clone(), Catalog::new(Locale::En));

    let output = tool
        .execute_tool(
            ToolRequest {
                run_id: "run-test".into(),
                turn_id: 1,
                tool_call_id: "tool-call-test".into(),
                tool_name: "agent.manifest.propose_patch".into(),
                arguments: json!({
                    "patch": {
                        "op": "enable_tool",
                        "toolName": "host.exec.start"
                    }
                }),
                state: AgentState::default(),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(store.pending_len(), 1);
    assert_eq!(store.approved_len(), 0);
    assert_eq!(output.details["proposalId"], "manifest-proposal-1");
    assert_eq!(output.details["summary"], "enable tool host.exec.start");
    assert!(matches!(&output.content[0], ContentBlock::Json { .. }));
}

#[tokio::test]
async fn manifest_proposal_does_not_apply_without_session_apply() {
    let store = ManifestProposalStore::default();
    let tool = ManifestPatchProposalTool::new(store.clone(), Catalog::new(Locale::En));

    tool.execute_tool(
        ToolRequest {
            run_id: "run-test".into(),
            turn_id: 1,
            tool_call_id: "tool-call-test".into(),
            tool_name: "agent.manifest.propose_patch".into(),
            arguments: json!({
                "patch": {
                    "op": "replace_system_prompt",
                    "prompt": "new prompt"
                }
            }),
            state: AgentState::default(),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(store.pending_len(), 1);
    assert_eq!(store.approved_len(), 0);
}
