use noloong_agent::{ApprovalPolicy, Catalog, Locale, ProductApprovalHook};
use noloong_agent_core::{
    BeforeToolCallContext, BeforeToolCallResult, CancellationToken, ToolCall, ToolCallHook,
    ToolPermissionOutcome, ToolSpec,
};

#[tokio::test]
async fn approval_host_exec_start_allow_deny() {
    let allow = ProductApprovalHook::new(ApprovalPolicy::AllowAll, Catalog::new(Locale::En))
        .before_tool_call(context("host.exec.start"), CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    let deny = ProductApprovalHook::new(
        ApprovalPolicy::AutoReview {
            fallback_to_human: false,
        },
        Catalog::new(Locale::En),
    )
    .before_tool_call(context("host.exec.start"), CancellationToken::new())
    .await
    .unwrap()
    .unwrap();

    assert!(matches!(
        allow,
        BeforeToolCallResult::Decision {
            decision
        } if decision.outcome == ToolPermissionOutcome::Allow
    ));
    assert!(matches!(
        deny,
        BeforeToolCallResult::Decision {
            decision
        } if decision.outcome == ToolPermissionOutcome::Deny
    ));
}

#[tokio::test]
async fn approval_auto_review_can_be_disabled_with_human_fallback() {
    let result = ProductApprovalHook::new(
        ApprovalPolicy::AutoReview {
            fallback_to_human: true,
        },
        Catalog::new(Locale::En),
    )
    .before_tool_call(context("host.exec.terminate"), CancellationToken::new())
    .await
    .unwrap()
    .unwrap();

    assert!(matches!(result, BeforeToolCallResult::Approval { .. }));
}

fn context(tool_name: &str) -> BeforeToolCallContext {
    BeforeToolCallContext {
        run_id: "run-test".into(),
        turn_id: 1,
        tool_call: ToolCall {
            id: "tool-call-test".into(),
            name: tool_name.into(),
            arguments: serde_json::json!({"command": "printf hello"}),
        },
        tool_spec: ToolSpec {
            name: tool_name.into(),
            description: String::new(),
            input_schema: serde_json::json!({}),
            execution_mode: None,
            permissions: Vec::new(),
        },
        state: Default::default(),
    }
}
