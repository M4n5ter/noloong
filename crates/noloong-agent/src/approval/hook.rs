use crate::{BuiltInToolName, Catalog};
use noloong_agent_core::{
    BeforeToolCallContext, BeforeToolCallResult, BoxFuture, CancellationToken,
    ToolApprovalRequestSpec, ToolCallHook,
};
use serde_json::json;
use std::sync::Arc;

use super::{
    cache::{
        ApprovalCache, ApprovalCacheKey, approval_cache_key_for_tool_call,
        host_exec_start_approval_input,
    },
    classification::{ApprovalClassification, classify_built_in_tool, classify_host_exec_start},
    constants::BUILT_IN_APPROVAL_HOOK_ID,
    decisions::{allow_decision, deny_decision},
    metadata::human_reviewer_tool_metadata,
    policy::ApprovalPolicy,
    reviewer::ApprovalReviewer,
};

#[derive(Clone)]
pub struct BuiltInApprovalHook {
    policy: ApprovalPolicy,
    catalog: Catalog,
    reviewer: Option<Arc<dyn ApprovalReviewer>>,
    approval_cache: Option<ApprovalCache>,
}

impl BuiltInApprovalHook {
    pub fn new(policy: ApprovalPolicy, catalog: Catalog) -> Self {
        Self {
            policy,
            catalog,
            reviewer: None,
            approval_cache: None,
        }
    }

    pub fn with_reviewer(mut self, reviewer: Arc<dyn ApprovalReviewer>) -> Self {
        self.reviewer = Some(reviewer);
        self
    }

    pub(crate) fn with_approval_cache(mut self, approval_cache: ApprovalCache) -> Self {
        self.approval_cache = Some(approval_cache);
        self
    }
}

impl ToolCallHook for BuiltInApprovalHook {
    fn id(&self) -> Option<&str> {
        Some(BUILT_IN_APPROVAL_HOOK_ID)
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
                policy => {
                    let classification = self.classify(&context);
                    match policy {
                        ApprovalPolicy::RequireApproval => {
                            self.result_from_classification(&context, classification)
                        }
                        ApprovalPolicy::AutoReview { fallback_to_human } => {
                            self.result_from_classification_with_auto_review(
                                context,
                                classification,
                                *fallback_to_human,
                                cancellation,
                            )
                            .await?
                        }
                        ApprovalPolicy::AllowAll => {
                            unreachable!("AllowAll handled before classification")
                        }
                    }
                }
            };
            Ok(Some(result))
        })
    }
}

impl BuiltInApprovalHook {
    fn classify(&self, context: &BeforeToolCallContext) -> ApprovalClassification {
        match BuiltInToolName::parse(&context.tool_call.name) {
            Ok(BuiltInToolName::HostExecStart) => {
                let input = host_exec_start_approval_input(&context.tool_call);
                let cache_key = input.as_ref().and_then(|input| input.cache_key.clone());
                if let Some(classification) = self.cached_classification(cache_key.as_ref()) {
                    return classification;
                }
                classify_host_exec_start(input.as_ref().map(|input| &input.input), cache_key)
            }
            Ok(tool_name) => {
                let cache_key = approval_cache_key_for_tool_call(&context.tool_call);
                if let Some(classification) = self.cached_classification(cache_key.as_ref()) {
                    return classification;
                }
                classify_built_in_tool(tool_name, cache_key)
            }
            Err(_) => ApprovalClassification::needs_approval(
                "unknown_tool",
                "unknown tools require approval",
                None,
            ),
        }
    }

    fn cached_classification(
        &self,
        cache_key: Option<&ApprovalCacheKey>,
    ) -> Option<ApprovalClassification> {
        if cache_key.is_some_and(|key| {
            self.approval_cache
                .as_ref()
                .is_some_and(|cache| cache.contains(key))
        }) {
            return Some(ApprovalClassification::allow(
                "session_cache",
                "tool call matches a previous session approval",
                cache_key.cloned(),
            ));
        }
        None
    }

    async fn result_from_classification_with_auto_review(
        &self,
        context: BeforeToolCallContext,
        classification: ApprovalClassification,
        fallback_to_human: bool,
        cancellation: CancellationToken,
    ) -> noloong_agent_core::Result<BeforeToolCallResult> {
        if let Some(result) = self.result_from_terminal_classification(&context, &classification) {
            return Ok(result);
        }
        let result = if let Some(reviewer) = &self.reviewer {
            BeforeToolCallResult::decision(reviewer.review_tool_call(context, cancellation).await?)
        } else if fallback_to_human {
            BeforeToolCallResult::approval(self.approval_request(
                &context,
                &classification,
                self.catalog.approval_auto_review_human_fallback_reason(),
            ))
        } else {
            BeforeToolCallResult::decision(deny_decision(
                self.catalog.approval_auto_review_denied_reason(),
                "policy",
                json!({"policy": "auto_review", "fallbackToHuman": false}),
            ))
        };
        Ok(result)
    }

    fn result_from_classification(
        &self,
        context: &BeforeToolCallContext,
        classification: ApprovalClassification,
    ) -> BeforeToolCallResult {
        if let Some(result) = self.result_from_terminal_classification(context, &classification) {
            return result;
        }
        BeforeToolCallResult::approval(self.approval_request(
            context,
            &classification,
            self.catalog.approval_human_required_reason(),
        ))
    }

    fn result_from_terminal_classification(
        &self,
        context: &BeforeToolCallContext,
        classification: &ApprovalClassification,
    ) -> Option<BeforeToolCallResult> {
        classification
            .terminal_decision(self.catalog.approval_allow_reason(), &context.tool_call)
            .map(BeforeToolCallResult::decision)
    }

    fn approval_request(
        &self,
        context: &BeforeToolCallContext,
        classification: &ApprovalClassification,
        reason: &str,
    ) -> ToolApprovalRequestSpec {
        ToolApprovalRequestSpec {
            prompt: Some(self.catalog.render_approval_prompt(&context.tool_call)),
            reason: Some(reason.into()),
            expires_at_ms: None,
            metadata: classification.metadata(human_reviewer_tool_metadata(&context.tool_call)),
        }
    }
}
