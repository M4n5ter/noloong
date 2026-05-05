use noloong_agent::{
    APPLY_PATCH_TOOL_NAME, ApprovalPolicy, ApprovalReviewer, BuiltInApprovalHook, BuiltInToolName,
    Catalog, HostExecStartTool, HostProcessManager, Locale, ManifestPatchProposalTool,
    ManifestProposalStore, WRITE_FILE_TOOL_NAME,
    approval::allow_decision as approval_allow_decision,
};
use noloong_agent_core::{
    BeforeToolCallContext, BeforeToolCallResult, CancellationToken, ToolCall, ToolCallHook,
    ToolPermissionDecision, ToolPermissionOutcome, ToolProvider, ToolSpec,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn approval_host_exec_start_allow_deny() {
    let allow = BuiltInApprovalHook::new(ApprovalPolicy::AllowAll, Catalog::new(Locale::En))
        .before_tool_call(
            context(BuiltInToolName::HostExecStart, command_args("printf hello")),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    let deny = BuiltInApprovalHook::new(
        ApprovalPolicy::AutoReview {
            fallback_to_human: false,
        },
        Catalog::new(Locale::En),
    )
    .before_tool_call(
        context(BuiltInToolName::HostExecStart, command_args("printf hello")),
        CancellationToken::new(),
    )
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
    let result = BuiltInApprovalHook::new(
        ApprovalPolicy::AutoReview {
            fallback_to_human: true,
        },
        Catalog::new(Locale::En),
    )
    .before_tool_call(
        context(
            BuiltInToolName::HostExecTerminate,
            serde_json::json!({"jobId": "job-1"}),
        ),
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .unwrap();

    assert!(matches!(result, BeforeToolCallResult::Approval { .. }));
}

#[tokio::test]
async fn require_approval_allows_safe_built_in_tools() {
    for tool_name in [
        BuiltInToolName::HostExecRead,
        BuiltInToolName::HostExecWait,
        BuiltInToolName::HostExecList,
    ] {
        let result =
            BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
                .before_tool_call(
                    context(tool_name, serde_json::json!({})),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .unwrap();

        assert!(matches!(
            result,
            BeforeToolCallResult::Decision {
                decision
            } if decision.outcome == ToolPermissionOutcome::Allow
        ));
    }
}

#[tokio::test]
async fn require_approval_decisions_include_classification_metadata() {
    let result =
        BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
            .before_tool_call(
                context(BuiltInToolName::HostExecStart, command_args("pwd")),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

    let BeforeToolCallResult::Decision { decision } = result else {
        panic!("safe command should be allowed");
    };
    assert_eq!(decision.metadata["classificationSource"], "host_command");
    assert_eq!(decision.metadata["classificationDecision"], "allow");
    assert_eq!(
        decision.metadata["toolName"],
        BuiltInToolName::HostExecStart.as_str()
    );
    assert_eq!(decision.metadata["toolCallId"], "tool-call-test");
}

#[tokio::test]
async fn require_approval_prompts_for_unknown_tools() {
    let result =
        BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
            .before_tool_call(
                BeforeToolCallContext {
                    run_id: "run-test".into(),
                    turn_id: 1,
                    tool_call: ToolCall {
                        id: "tool-call-unknown".into(),
                        name: "external.tool".into(),
                        arguments: serde_json::json!({}),
                    },
                    tool_spec: ToolSpec {
                        name: "external.tool".into(),
                        description: String::new(),
                        input_schema: serde_json::json!({}),
                        execution_mode: None,
                        permissions: Vec::new(),
                    },
                    state: Default::default(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

    assert!(matches!(result, BeforeToolCallResult::Approval { .. }));
}

#[tokio::test]
async fn require_approval_prompts_for_control_and_manifest_tools() {
    for (tool_name, arguments) in [
        (
            BuiltInToolName::HostExecWrite,
            serde_json::json!({"jobId": "job-1", "text": "x"}),
        ),
        (
            BuiltInToolName::HostExecTerminate,
            serde_json::json!({"jobId": "job-1"}),
        ),
        (
            BuiltInToolName::ManifestProposePatch,
            serde_json::json!({"patch": {"op": "set_locale", "locale": "en"}}),
        ),
    ] {
        let result =
            BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
                .before_tool_call(context(tool_name, arguments), CancellationToken::new())
                .await
                .unwrap()
                .unwrap();

        assert!(matches!(result, BeforeToolCallResult::Approval { .. }));
    }
}

#[tokio::test]
async fn require_approval_prompts_for_file_edit_tools_with_metadata() {
    for (tool_name, arguments, expected_paths) in [
        (
            WRITE_FILE_TOOL_NAME,
            serde_json::json!({
                "path": "src/lib.rs",
                "content": "new content"
            }),
            vec!["src/lib.rs"],
        ),
        (
            APPLY_PATCH_TOOL_NAME,
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** Add File: src/new.rs\n+new\n*** End Patch"
            }),
            vec!["src/lib.rs", "src/new.rs"],
        ),
    ] {
        let result =
            BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
                .before_tool_call(raw_context(tool_name, arguments), CancellationToken::new())
                .await
                .unwrap()
                .unwrap();

        let BeforeToolCallResult::Approval { approval } = result else {
            panic!("{tool_name} should require approval");
        };
        assert_eq!(approval.metadata["classificationSource"], "file_edit");
        assert_eq!(
            approval.metadata["classificationDecision"],
            "needs_approval"
        );
        assert_eq!(approval.metadata["builtIn"], true);
        assert_eq!(approval.metadata["capability"], "host.file.write");
        assert_eq!(approval.metadata["tool"], tool_name);
        assert_eq!(
            approval.metadata["targetPaths"],
            serde_json::json!(expected_paths)
        );
        assert!(approval.metadata.get("approvalCacheKey").is_none());
    }
}

#[tokio::test]
async fn malformed_file_edit_arguments_still_require_approval() {
    let result =
        BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En))
            .before_tool_call(
                raw_context(WRITE_FILE_TOOL_NAME, serde_json::json!({})),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

    assert!(matches!(result, BeforeToolCallResult::Approval { .. }));
}

#[tokio::test]
async fn require_approval_classifies_host_exec_start_commands() {
    let hook = BuiltInApprovalHook::new(ApprovalPolicy::RequireApproval, Catalog::new(Locale::En));

    for command in [
        "pwd",
        "ls -la",
        "rg foo src",
        "grep -R foo src",
        "head -n 20 Cargo.toml",
        "tail -n 20 Cargo.toml",
        "wc -l Cargo.toml",
        "sed -n '1,10p' Cargo.toml",
        "git status --short",
        "git log --oneline",
        "git diff",
        "git show HEAD:Cargo.toml",
        "git branch --show-current",
        "rg foo src | head -n 20",
    ] {
        let result = hook
            .before_tool_call(
                context(BuiltInToolName::HostExecStart, command_args(command)),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

        assert!(
            matches!(result, BeforeToolCallResult::Decision { decision }
                if decision.outcome == ToolPermissionOutcome::Allow),
            "{command} should be allowed"
        );
    }

    for command in [
        "python -c 'print(1)'",
        "node -e 'console.log(1)'",
        "curl https://example.com",
        "rm -rf target",
        "sudo rm -rf /tmp/x",
        "git -C /tmp status",
        "echo $(pwd)",
    ] {
        let result = hook
            .before_tool_call(
                context(BuiltInToolName::HostExecStart, command_args(command)),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();

        assert!(
            matches!(result, BeforeToolCallResult::Approval { .. }),
            "{command} should require approval"
        );
    }
}

#[tokio::test]
async fn auto_review_only_runs_when_classification_requires_approval() {
    let reviewer = Arc::new(CountingReviewer::default());
    let reviewer_for_hook: Arc<CountingReviewer> = Arc::clone(&reviewer);
    let reviewer_for_hook: Arc<dyn ApprovalReviewer> = reviewer_for_hook;
    let hook = BuiltInApprovalHook::new(
        ApprovalPolicy::AutoReview {
            fallback_to_human: false,
        },
        Catalog::new(Locale::En),
    )
    .with_reviewer(reviewer_for_hook);

    let safe = hook
        .before_tool_call(
            context(BuiltInToolName::HostExecStart, command_args("pwd")),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        safe,
        BeforeToolCallResult::Decision {
            decision
        } if decision.outcome == ToolPermissionOutcome::Allow
    ));
    assert_eq!(reviewer.calls(), 0);

    let needs_review = hook
        .before_tool_call(
            context(
                BuiltInToolName::HostExecStart,
                command_args("python -c 'print(1)'"),
            ),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        needs_review,
        BeforeToolCallResult::Decision {
            decision
        } if decision.outcome == ToolPermissionOutcome::Allow
    ));
    assert_eq!(reviewer.calls(), 1);
}

#[test]
fn built_in_tool_specs_include_permission_metadata() {
    let start = HostExecStartTool::new(HostProcessManager::new(), Catalog::new(Locale::En));
    let start_spec = start.spec();
    assert_eq!(start_spec.permissions[0].capability, "host.command");
    assert_eq!(start_spec.permissions[0].metadata["builtIn"], true);
    assert_eq!(
        start_spec.permissions[0].metadata["tool"],
        BuiltInToolName::HostExecStart.as_str()
    );

    let manifest =
        ManifestPatchProposalTool::new(ManifestProposalStore::default(), Catalog::new(Locale::En));
    let manifest_spec = manifest.spec();
    assert_eq!(
        manifest_spec.permissions[0].capability,
        "agent.manifest.patch"
    );
    assert_eq!(manifest_spec.permissions[0].metadata["builtIn"], true);
}

#[derive(Default)]
struct CountingReviewer {
    calls: AtomicUsize,
}

impl CountingReviewer {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ApprovalReviewer for CountingReviewer {
    fn review_tool_call<'a>(
        &'a self,
        _context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> noloong_agent_core::BoxFuture<'a, ToolPermissionDecision> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(approval_allow_decision(
                "test reviewer",
                "test",
                serde_json::json!({}),
            ))
        })
    }
}

fn context(tool_name: BuiltInToolName, arguments: serde_json::Value) -> BeforeToolCallContext {
    let tool_name = tool_name.as_str();
    raw_context(tool_name, arguments)
}

fn raw_context(tool_name: &str, arguments: serde_json::Value) -> BeforeToolCallContext {
    BeforeToolCallContext {
        run_id: "run-test".into(),
        turn_id: 1,
        tool_call: ToolCall {
            id: "tool-call-test".into(),
            name: tool_name.into(),
            arguments,
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

fn command_args(command: &str) -> serde_json::Value {
    serde_json::json!({"command": command, "shell": "sh"})
}
