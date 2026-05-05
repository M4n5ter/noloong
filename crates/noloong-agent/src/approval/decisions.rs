use noloong_agent_core::{ToolPermissionDecision, ToolPermissionOutcome};
use serde_json::Value;

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
