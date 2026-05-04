use super::{
    AgentState, ContentBlock, MediaDelta, ThinkingDelta, ToolApprovalRequestSpec, ToolCall,
    ToolOutput, ToolPermissionDecision, ToolSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStreamEvent {
    Started {
        stream_id: String,
    },
    ThinkingDelta {
        #[serde(flatten)]
        delta: ThinkingDelta,
    },
    TextDelta {
        text: String,
    },
    MediaDelta {
        #[serde(flatten)]
        delta: MediaDelta,
    },
    ToolCall {
        tool_call: ToolCall,
    },
    Finished {
        stop_reason: StopReason,
    },
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeToolCallContext {
    pub run_id: String,
    pub turn_id: u64,
    pub tool_call: ToolCall,
    pub tool_spec: ToolSpec,
    pub state: AgentState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BeforeToolCallResult {
    Decision { decision: ToolPermissionDecision },
    Approval { approval: ToolApprovalRequestSpec },
}

impl BeforeToolCallResult {
    pub fn decision(decision: ToolPermissionDecision) -> Self {
        Self::Decision { decision }
    }

    pub fn approval(approval: ToolApprovalRequestSpec) -> Self {
        Self::Approval { approval }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterToolCallContext {
    pub run_id: String,
    pub turn_id: u64,
    pub tool_call: ToolCall,
    pub output: ToolOutput,
    pub state: AgentState,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterToolCallResult {
    #[serde(default)]
    pub content: Option<Vec<ContentBlock>>,
    #[serde(default)]
    pub details: Option<Value>,
    #[serde(default)]
    pub is_error: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}
