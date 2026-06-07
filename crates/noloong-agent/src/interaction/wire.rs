use super::InteractionError;
use crate::{AgentManifest, ManifestPatch};
use noloong_agent_core::{
    AgentEvent, AgentMessage, AgentState, RunStatus, ToolApprovalId, ToolApprovalRequest,
    ToolCallId, ToolOutput, ToolPermissionDecision, ToolUpdate,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionAuthorityCapability {
    AgentRun,
    AgentQueue,
    ApprovalResolve,
    ManifestApply,
    ProcessControl,
    SubagentSpawn,
    GoalManage,
    AutomationManage,
    SessionDelete,
}

impl InteractionAuthorityCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentRun => "agent.run",
            Self::AgentQueue => "agent.queue",
            Self::ApprovalResolve => "approval.resolve",
            Self::ManifestApply => "manifest.apply",
            Self::ProcessControl => "process.control",
            Self::SubagentSpawn => "subagent.spawn",
            Self::GoalManage => "goal.manage",
            Self::AutomationManage => "automation.manage",
            Self::SessionDelete => "session.delete",
        }
    }

    pub fn parse(value: &str) -> Result<Self, InteractionError> {
        match value {
            "agent.run" => Ok(Self::AgentRun),
            "agent.queue" => Ok(Self::AgentQueue),
            "approval.resolve" => Ok(Self::ApprovalResolve),
            "manifest.apply" => Ok(Self::ManifestApply),
            "process.control" => Ok(Self::ProcessControl),
            "subagent.spawn" => Ok(Self::SubagentSpawn),
            "goal.manage" => Ok(Self::GoalManage),
            "automation.manage" => Ok(Self::AutomationManage),
            "session.delete" => Ok(Self::SessionDelete),
            other => Err(InteractionError::invalid_params(format!(
                "unknown authority capability: {other}"
            ))),
        }
    }

    pub fn all() -> BTreeSet<Self> {
        [
            Self::AgentRun,
            Self::AgentQueue,
            Self::ApprovalResolve,
            Self::ManifestApply,
            Self::ProcessControl,
            Self::SubagentSpawn,
            Self::GoalManage,
            Self::AutomationManage,
            Self::SessionDelete,
        ]
        .into_iter()
        .collect()
    }
}

impl Serialize for InteractionAuthorityCapability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InteractionAuthorityCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionUxCapabilities {
    #[serde(default)]
    pub raw_events: bool,
    #[serde(default)]
    pub display_events: bool,
    #[serde(default)]
    pub stream_text: bool,
    #[serde(default)]
    pub edit_message: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_message_bytes: Option<usize>,
}

impl InteractionUxCapabilities {
    pub fn all() -> Self {
        Self {
            raw_events: true,
            display_events: true,
            stream_text: true,
            edit_message: true,
            markdown: true,
            max_message_bytes: None,
        }
    }

    pub fn grant(&self, requested: &Self) -> Self {
        Self {
            raw_events: self.raw_events && requested.raw_events,
            display_events: self.display_events && requested.display_events,
            stream_text: self.stream_text && requested.stream_text,
            edit_message: self.edit_message && requested.edit_message,
            markdown: self.markdown && requested.markdown,
            max_message_bytes: granted_max_bytes(
                self.max_message_bytes,
                requested.max_message_bytes,
            ),
        }
    }
}

fn granted_max_bytes(allowed: Option<usize>, requested: Option<usize>) -> Option<usize> {
    match (allowed, requested) {
        (Some(allowed), Some(requested)) => Some(allowed.min(requested)),
        (Some(allowed), None) => Some(allowed),
        (None, Some(requested)) => Some(requested),
        (None, None) => None,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub requested_authority: BTreeSet<InteractionAuthorityCapability>,
    #[serde(default)]
    pub requested_ux: InteractionUxCapabilities,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionCapabilityGrant {
    #[serde(default)]
    pub authority: BTreeSet<InteractionAuthorityCapability>,
    #[serde(default)]
    pub ux: InteractionUxCapabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionCapabilityPolicy {
    #[serde(default)]
    pub allowed_authority: BTreeSet<InteractionAuthorityCapability>,
    #[serde(default)]
    pub allowed_ux: InteractionUxCapabilities,
}

impl InteractionCapabilityPolicy {
    pub fn allow_all() -> Self {
        Self {
            allowed_authority: InteractionAuthorityCapability::all(),
            allowed_ux: InteractionUxCapabilities::all(),
        }
    }

    pub fn grant(&self, client: &InteractionClientInfo) -> InteractionCapabilityGrant {
        let authority = client
            .requested_authority
            .intersection(&self.allowed_authority)
            .copied()
            .collect();
        InteractionCapabilityGrant {
            authority,
            ux: self.allowed_ux.grant(&client.requested_ux),
        }
    }

    pub fn authorize(
        grant: &InteractionCapabilityGrant,
        method: &str,
        required: InteractionAuthorityCapability,
    ) -> Result<(), InteractionError> {
        if grant.authority.contains(&required) {
            return Ok(());
        }
        Err(InteractionError::unauthorized(
            method,
            required.as_str(),
            format!("method {method} requires {}", required.as_str()),
        ))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionProfileDescriptor {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub default_manifest_patches: Vec<ManifestPatch>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionSessionStatus {
    Idle,
    Running,
    Completed,
    Aborted,
    Failed,
    Paused,
}

impl From<RunStatus> for InteractionSessionStatus {
    fn from(value: RunStatus) -> Self {
        match value {
            RunStatus::Idle => Self::Idle,
            RunStatus::Running => Self::Running,
            RunStatus::Completed => Self::Completed,
            RunStatus::Aborted => Self::Aborted,
            RunStatus::Failed => Self::Failed,
            RunStatus::Paused => Self::Paused,
        }
    }
}

impl InteractionSessionStatus {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }

    pub(crate) const fn is_settled(&self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSessionDescriptor {
    pub session_id: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: InteractionSessionStatus,
    pub manifest: AgentManifest,
    pub state: AgentState,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DisplayEvent {
    RunStarted {
        run_id: String,
    },
    RunCompleted {
        run_id: String,
    },
    RunAborted {
        run_id: String,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
    RunPaused {
        run_id: String,
        reason: Value,
    },
    ThoughtStarted {
        run_id: String,
        thought_id: String,
    },
    ThoughtDelta {
        run_id: String,
        thought_id: String,
        kind: String,
        text: String,
    },
    ThoughtCompleted {
        run_id: String,
        thought_id: String,
        elapsed_ms: u64,
    },
    AssistantMessageDelta {
        run_id: String,
        display_message_id: String,
        text: String,
    },
    AssistantMessageFinal {
        run_id: String,
        display_message_id: String,
        message: AgentMessage,
        #[serde(default)]
        truncated: bool,
    },
    ToolStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
    },
    ToolUpdated {
        tool_call_id: ToolCallId,
        update: ToolUpdate,
    },
    ToolCompleted {
        tool_call_id: ToolCallId,
        output: ToolOutput,
    },
    ApprovalRequested {
        approval: ToolApprovalRequest,
    },
    ApprovalResolved {
        approval_id: ToolApprovalId,
        decision: ToolPermissionDecision,
    },
    ApprovalExpired {
        approval_id: ToolApprovalId,
        decision: ToolPermissionDecision,
    },
    RawEvent {
        event: AgentEvent,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum JsonRpcResponsePayload {
    Result { result: Value },
    Error { error: JsonRpcErrorObject },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(flatten)]
    pub payload: JsonRpcResponsePayload,
}

impl JsonRpcResponse {
    pub fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            payload: JsonRpcResponsePayload::Result { result },
        }
    }

    pub fn error(id: Value, error: InteractionError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            payload: JsonRpcResponsePayload::Error {
                error: JsonRpcErrorObject::from(error),
            },
        }
    }

    pub fn parse_error(error: InteractionError) -> Self {
        Self::error(Value::Null, error)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<InteractionError> for JsonRpcErrorObject {
    fn from(error: InteractionError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            data: error.data,
        }
    }
}
