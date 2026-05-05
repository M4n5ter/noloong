mod cache;
mod command_safety;

use crate::{BuiltInToolName, Catalog};
use cache::{
    ApprovalCacheKey, approval_cache_key_for_tool_call, host_exec_start_cache_key,
    parse_start_command_request,
};
use command_safety::{CommandSafety, classify_host_command};
use noloong_agent_core::{
    BeforeToolCallContext, BeforeToolCallResult, BoxFuture, CancellationToken,
    ToolApprovalRequestSpec, ToolCall, ToolCallHook, ToolPermissionDecision, ToolPermissionOutcome,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) use cache::{ApprovalCache, cache_key_from_approval_resolution};

pub(crate) const BUILT_IN_APPROVAL_HOOK_ID: &str = "noloong.builtin.approval";
const APPROVAL_CACHE_KEY_METADATA: &str = "approvalCacheKey";

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
                let input = parse_start_command_request(&context.tool_call.arguments);
                let cache_key = input.as_ref().and_then(host_exec_start_cache_key);
                if let Some(classification) = self.cached_classification(cache_key.as_ref()) {
                    return classification;
                }
                classify_host_exec_start(input.as_ref(), cache_key)
            }
            Ok(tool_name) => {
                let cache_key = approval_cache_key_for_tool_call(&context.tool_call);
                if let Some(classification) = self.cached_classification(cache_key.as_ref()) {
                    return classification;
                }
                self.classify_built_in_tool(tool_name, cache_key)
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

    fn classify_built_in_tool(
        &self,
        tool_name: BuiltInToolName,
        cache_key: Option<ApprovalCacheKey>,
    ) -> ApprovalClassification {
        match tool_name {
            BuiltInToolName::HostExecRead
            | BuiltInToolName::HostExecWait
            | BuiltInToolName::HostExecList => ApprovalClassification::allow(
                "built_in_tool",
                "read-only host command lifecycle operation",
                cache_key,
            ),
            BuiltInToolName::HostExecWrite | BuiltInToolName::HostExecTerminate => {
                ApprovalClassification::needs_approval(
                    "built_in_tool",
                    "host command control operation requires approval",
                    cache_key,
                )
            }
            BuiltInToolName::ManifestProposePatch => ApprovalClassification::needs_approval(
                "built_in_tool",
                "manifest changes require approval",
                cache_key,
            ),
            BuiltInToolName::HostExecStart => {
                unreachable!("host.exec.start is classified before generic built-in tools")
            }
        }
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
        match classification.decision {
            ApprovalClass::Allow => Some(BeforeToolCallResult::decision(
                classification
                    .allow_decision(self.catalog.approval_allow_reason(), &context.tool_call),
            )),
            ApprovalClass::Deny => Some(BeforeToolCallResult::decision(
                classification.deny_decision(&context.tool_call),
            )),
            ApprovalClass::NeedsApproval => None,
        }
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
            metadata: classification.metadata(json!({
                "reviewer": "human",
                "toolName": context.tool_call.name,
                "toolCallId": context.tool_call.id,
            })),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApprovalClass {
    Allow,
    NeedsApproval,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ApprovalClassification {
    decision: ApprovalClass,
    source: &'static str,
    reason: &'static str,
    cache_key: Option<ApprovalCacheKey>,
}

impl ApprovalClassification {
    fn allow(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::Allow,
            source,
            reason,
            cache_key,
        }
    }

    fn needs_approval(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::NeedsApproval,
            source,
            reason,
            cache_key,
        }
    }

    fn deny(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::Deny,
            source,
            reason,
            cache_key,
        }
    }

    fn allow_decision(
        &self,
        default_reason: &'static str,
        tool_call: &ToolCall,
    ) -> ToolPermissionDecision {
        allow_decision(
            default_reason,
            "policy",
            self.metadata(tool_metadata(tool_call)),
        )
    }

    fn deny_decision(&self, tool_call: &ToolCall) -> ToolPermissionDecision {
        deny_decision(
            self.reason,
            "policy",
            self.metadata(tool_metadata(tool_call)),
        )
    }

    fn metadata(&self, mut base: Value) -> Value {
        let Value::Object(ref mut map) = base else {
            return base;
        };
        map.insert("classificationSource".into(), json!(self.source));
        map.insert("classificationReason".into(), json!(self.reason));
        map.insert(
            "classificationDecision".into(),
            json!(self.decision.as_str()),
        );
        if let Some(cache_key) = &self.cache_key {
            map.insert(
                APPROVAL_CACHE_KEY_METADATA.into(),
                json!(cache_key.as_str()),
            );
        }
        base
    }
}

impl ApprovalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::NeedsApproval => "needs_approval",
            Self::Deny => "deny",
        }
    }
}

fn tool_metadata(tool_call: &ToolCall) -> Value {
    json!({
        "toolName": tool_call.name,
        "toolCallId": tool_call.id,
    })
}

fn classify_host_exec_start(
    input: Option<&crate::StartCommandRequest>,
    cache_key: Option<ApprovalCacheKey>,
) -> ApprovalClassification {
    let Some(input) = input else {
        return ApprovalClassification::needs_approval(
            "host_command",
            "host command arguments could not be classified",
            cache_key,
        );
    };
    if input.command.trim().is_empty() {
        return ApprovalClassification::deny("host_command", "host command is empty", cache_key);
    }
    match classify_host_command(&input.command, input.shell.as_deref()) {
        CommandSafety::Safe => ApprovalClassification::allow(
            "host_command",
            "known safe read-only host command",
            cache_key,
        ),
        CommandSafety::Dangerous => ApprovalClassification::needs_approval(
            "host_command",
            "dangerous host command requires approval",
            cache_key,
        ),
        CommandSafety::Unknown => ApprovalClassification::needs_approval(
            "host_command",
            "unknown host command requires approval",
            cache_key,
        ),
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
