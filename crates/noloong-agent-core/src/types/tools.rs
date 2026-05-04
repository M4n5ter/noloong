use super::{ContentBlock, RunId, ToolApprovalId, ToolCallId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub execution_mode: Option<ToolExecutionMode>,
    #[serde(default)]
    pub permissions: Vec<ToolPermissionRequirement>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionRequirement {
    pub capability: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionOutcome {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionDecision {
    pub outcome: ToolPermissionOutcome,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub approver: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionDecisionRecord {
    #[serde(default)]
    pub hook_id: Option<String>,
    pub decision: ToolPermissionDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionAudit {
    pub tool_call: ToolCall,
    #[serde(default)]
    pub permissions: Vec<ToolPermissionRequirement>,
    #[serde(default)]
    pub decisions: Vec<ToolPermissionDecisionRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default = "empty_json_object")]
    pub metadata: Value,
}

fn empty_json_object() -> Value {
    Value::Object(Map::new())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub approval_id: ToolApprovalId,
    pub tool_call: ToolCall,
    #[serde(default)]
    pub permissions: Vec<ToolPermissionRequirement>,
    #[serde(default)]
    pub hook_id: Option<String>,
    pub request: ToolApprovalRequestSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResolution {
    pub approval_id: ToolApprovalId,
    pub decision: ToolPermissionDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalContinuation {
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub phase: String,
    pub scratch: crate::PhaseScratch,
    pub tool_execution_mode: ToolExecutionMode,
    #[serde(default)]
    pub preflights: Vec<ToolApprovalPreflight>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalPreflight {
    pub tool_call: ToolCall,
    pub permission_audit: ToolPermissionAudit,
    pub status: ToolApprovalPreflightStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolApprovalPreflightStatus {
    Ready,
    Denied {
        decision: ToolPermissionDecision,
    },
    Pending {
        approval_id: ToolApprovalId,
        hook_index: usize,
        #[serde(default)]
        hook_id: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunPauseReason {
    ToolApproval {
        continuation: ToolApprovalContinuation,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunResumeReason {
    ToolApproval { approval_ids: Vec<ToolApprovalId> },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub details: Value,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub updates: Vec<ToolUpdate>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolUpdate {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub details: Value,
}
