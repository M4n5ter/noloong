use crate::Catalog;
use noloong_agent_core::{
    BeforeToolCallContext, BeforeToolCallResult, BoxFuture, CancellationToken,
    ToolApprovalRequestSpec, ToolCallHook, ToolPermissionDecision, ToolPermissionOutcome,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ApprovalPolicy {
    AllowAll,
    #[default]
    RequireApproval,
    AutoReview {
        fallback_to_human: bool,
    },
}

pub trait ApprovalReviewer: Send + Sync {
    fn review_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolPermissionDecision>;
}

#[derive(Clone)]
pub struct BuiltInApprovalHook {
    policy: ApprovalPolicy,
    catalog: Catalog,
    reviewer: Option<Arc<dyn ApprovalReviewer>>,
}

impl BuiltInApprovalHook {
    pub fn new(policy: ApprovalPolicy, catalog: Catalog) -> Self {
        Self {
            policy,
            catalog,
            reviewer: None,
        }
    }

    pub fn with_reviewer(mut self, reviewer: Arc<dyn ApprovalReviewer>) -> Self {
        self.reviewer = Some(reviewer);
        self
    }
}

impl ToolCallHook for BuiltInApprovalHook {
    fn id(&self) -> Option<&str> {
        Some("noloong.builtin.approval")
    }

    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            let result = match &self.policy {
                ApprovalPolicy::AllowAll => BeforeToolCallResult::decision(allow_decision(
                    self.catalog.approval_allow_reason(),
                    "policy",
                    json!({"policy": "allow_all"}),
                )),
                ApprovalPolicy::RequireApproval => BeforeToolCallResult::approval(
                    self.approval_request(&context, self.catalog.approval_human_required_reason()),
                ),
                ApprovalPolicy::AutoReview { fallback_to_human } => {
                    if let Some(reviewer) = &self.reviewer {
                        BeforeToolCallResult::decision(
                            reviewer
                                .review_tool_call(context, cancellation.clone())
                                .await?,
                        )
                    } else if *fallback_to_human {
                        BeforeToolCallResult::approval(self.approval_request(
                            &context,
                            self.catalog.approval_auto_review_human_fallback_reason(),
                        ))
                    } else {
                        BeforeToolCallResult::decision(deny_decision(
                            self.catalog.approval_auto_review_denied_reason(),
                            "policy",
                            json!({"policy": "auto_review", "fallbackToHuman": false}),
                        ))
                    }
                }
            };
            Ok(Some(result))
        })
    }
}

impl BuiltInApprovalHook {
    fn approval_request(
        &self,
        context: &BeforeToolCallContext,
        reason: &str,
    ) -> ToolApprovalRequestSpec {
        ToolApprovalRequestSpec {
            prompt: Some(self.catalog.render_approval_prompt(&context.tool_call)),
            reason: Some(reason.into()),
            expires_at_ms: None,
            metadata: json!({
                "reviewer": "human",
                "toolName": context.tool_call.name,
                "toolCallId": context.tool_call.id,
            }),
        }
    }
}

pub fn allow_decision(
    reason: impl Into<String>,
    approver: impl Into<String>,
    metadata: Value,
) -> ToolPermissionDecision {
    ToolPermissionDecision {
        outcome: ToolPermissionOutcome::Allow,
        reason: Some(reason.into()),
        approver: Some(approver.into()),
        metadata,
    }
}

pub fn deny_decision(
    reason: impl Into<String>,
    approver: impl Into<String>,
    metadata: Value,
) -> ToolPermissionDecision {
    ToolPermissionDecision {
        outcome: ToolPermissionOutcome::Deny,
        reason: Some(reason.into()),
        approver: Some(approver.into()),
        metadata,
    }
}
