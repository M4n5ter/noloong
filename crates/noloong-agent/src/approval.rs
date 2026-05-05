use crate::{Catalog, MessageKey};
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
pub struct ProductApprovalHook {
    policy: ApprovalPolicy,
    catalog: Catalog,
    reviewer: Option<Arc<dyn ApprovalReviewer>>,
}

impl ProductApprovalHook {
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

impl ToolCallHook for ProductApprovalHook {
    fn id(&self) -> Option<&str> {
        Some("noloong.product.approval")
    }

    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            let result = match &self.policy {
                ApprovalPolicy::AllowAll => BeforeToolCallResult::decision(allow_decision(
                    "allowed by product approval policy",
                    "policy",
                    json!({"policy": "allow_all"}),
                )),
                ApprovalPolicy::RequireApproval => BeforeToolCallResult::approval(
                    self.approval_request(&context, "human approval required"),
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
                            "auto-review is disabled; human approval required",
                        ))
                    } else {
                        BeforeToolCallResult::decision(deny_decision(
                            "auto-review is disabled and human fallback is disabled",
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

impl ProductApprovalHook {
    fn approval_request(
        &self,
        context: &BeforeToolCallContext,
        reason: &str,
    ) -> ToolApprovalRequestSpec {
        ToolApprovalRequestSpec {
            prompt: Some(format!(
                "{} Tool: `{}`. Arguments: {}",
                self.catalog.message(MessageKey::ApprovalPrompt),
                context.tool_call.name,
                context.tool_call.arguments
            )),
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
