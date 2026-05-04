use super::{
    AgentMessage, EventSequence, MessageId, ModelStreamEvent, RunId, RunPauseReason,
    RunResumeReason, ToolApprovalId, ToolApprovalRequest, ToolCall, ToolCallId, ToolOutput,
    ToolPermissionDecision, ToolPermissionRequirement, ToolSpec, ToolUpdate, TurnId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub sequence: EventSequence,
    pub run_id: RunId,
    pub turn_id: Option<TurnId>,
    pub phase: Option<String>,
    pub kind: AgentEventKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventKind {
    RunStarted,
    RunCompleted,
    RunAborted,
    RunFailed {
        error: String,
    },
    TurnStarted,
    TurnCompleted {
        decision: TurnDecision,
    },
    PhaseStarted {
        phase: String,
    },
    PhaseCompleted {
        phase: String,
    },
    PhaseFailed {
        phase: String,
        error: String,
    },
    EffectProposed {
        effect: AgentEffect,
    },
    EffectCommitted {
        effect: AgentEffect,
    },
    EffectRejected {
        effect: AgentEffect,
        reason: String,
    },
    ModelStreamEvent {
        provider: String,
        event: ModelStreamEvent,
    },
    ToolCallResolved {
        tool_call: ToolCall,
    },
    ToolPermissionRequested {
        tool_call: ToolCall,
        permissions: Vec<ToolPermissionRequirement>,
    },
    ToolPermissionDecided {
        tool_call_id: ToolCallId,
        tool_name: String,
        hook_id: Option<String>,
        decision: ToolPermissionDecision,
    },
    ToolApprovalRequested {
        approval: ToolApprovalRequest,
    },
    ToolApprovalResolved {
        approval_id: ToolApprovalId,
        decision: ToolPermissionDecision,
    },
    ToolApprovalExpired {
        approval_id: ToolApprovalId,
        decision: ToolPermissionDecision,
    },
    ToolExecutionStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
    },
    ToolExecutionUpdate {
        tool_call_id: ToolCallId,
        update: ToolUpdate,
    },
    ToolExecutionCompleted {
        tool_call_id: ToolCallId,
        output: ToolOutput,
    },
    RunPaused {
        reason: Box<RunPauseReason>,
    },
    RunResumed {
        reason: RunResumeReason,
    },
    ExtensionEvent {
        extension: String,
        payload: Value,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEffect {
    AppendMessage { message: AgentMessage },
    PatchContext { patch: ContextPatch },
    SetAvailableTools { tools: Vec<ToolSpec> },
    CompactMessages { compaction: MessageCompaction },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ContextPatch {
    Set { key: String, value: Value },
    Remove { key: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageCompaction {
    pub summary_message: AgentMessage,
    #[serde(default)]
    pub retained_message_ids: Vec<MessageId>,
    #[serde(default)]
    pub dropped_message_ids: Vec<MessageId>,
    pub tokens_before: u64,
    pub tokens_after: u64,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentState {
    pub run_id: Option<RunId>,
    pub status: RunStatus,
    pub messages: Vec<AgentMessage>,
    pub context: BTreeMap<String, Value>,
    pub available_tools: BTreeMap<String, ToolSpec>,
    #[serde(default)]
    pub pending_tool_approvals: BTreeMap<ToolApprovalId, ToolApprovalRequest>,
    pub active_phase: Option<String>,
    pub completed_turns: u64,
    pub last_error: Option<String>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            run_id: None,
            status: RunStatus::Idle,
            messages: Vec::new(),
            context: BTreeMap::new(),
            available_tools: BTreeMap::new(),
            pending_tool_approvals: BTreeMap::new(),
            active_phase: None,
            completed_turns: 0,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    Completed,
    Aborted,
    Failed,
    Paused,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnDecision {
    Continue,
    Stop,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    #[default]
    All,
    OneAtATime,
}
