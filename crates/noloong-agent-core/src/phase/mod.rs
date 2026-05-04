mod assistant;
mod hooks;
mod standard;
mod tool;

use crate::{
    AgentEffect, AgentMessage, AgentState, ModelRequest, ModelStreamEvent, RunPauseReason,
    ToolApprovalRequest, ToolCall, ToolOutput, ToolPermissionAudit, TurnDecision,
    providers::{BoxFuture, CancellationToken, ModelStreamSink},
};
use serde::{Deserialize, Serialize};

pub(crate) use tool::resume_tool_approval_continuation;

pub const PHASE_INPUT_INGEST: &str = "input.ingest";
pub const PHASE_CONTEXT_PREPARE: &str = "context.prepare";
pub const PHASE_CONTEXT_COMPACT: &str = "context.compact";
pub const PHASE_MODEL_REQUEST_PREPARE: &str = "model.request.prepare";
pub const PHASE_MODEL_STREAM: &str = "model.stream";
pub const PHASE_ASSISTANT_COMMIT: &str = "assistant.commit";
pub const PHASE_TOOL_CALL_RESOLVE: &str = "tool.call.resolve";
pub const PHASE_TOOL_EXECUTE: &str = "tool.execute";
pub const PHASE_TURN_DECISION: &str = "turn.decision";

pub trait PhaseNode: Send + Sync {
    fn id(&self) -> &str;
    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput>;
}

pub struct PhaseContext<'a> {
    pub runtime: &'a crate::AgentRuntime,
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: AgentState,
    pub scratch: PhaseScratch,
    pub cancellation: CancellationToken,
    pub model_stream_sink: Option<ModelStreamSink>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhaseScratch {
    #[serde(default)]
    pub input: Option<AgentMessage>,
    #[serde(default)]
    pub model_request: Option<ModelRequest>,
    #[serde(default)]
    pub request_messages_override: Option<Vec<AgentMessage>>,
    #[serde(default)]
    pub model_events: Vec<ModelStreamEvent>,
    #[serde(default)]
    pub assistant_message: Option<AgentMessage>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub decision: Option<TurnDecision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseOutput {
    #[serde(default)]
    pub scratch: PhaseScratch,
    #[serde(default)]
    pub effects: Vec<AgentEffect>,
    #[serde(default)]
    pub stream_events: Vec<ModelStreamEvent>,
    #[serde(default)]
    pub resolved_tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub completed_tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub tool_permission_audits: Vec<ToolPermissionAudit>,
    #[serde(default)]
    pub completed_tool_permission_audits: Vec<ToolPermissionAudit>,
    #[serde(default)]
    pub tool_approval_requests: Vec<ToolApprovalRequest>,
    #[serde(default)]
    pub pause: Option<RunPauseReason>,
}

impl PhaseOutput {
    pub fn from_scratch(scratch: PhaseScratch) -> Self {
        Self {
            scratch,
            effects: Vec::new(),
            stream_events: Vec::new(),
            resolved_tool_calls: Vec::new(),
            tool_outputs: Vec::new(),
            completed_tool_outputs: Vec::new(),
            tool_permission_audits: Vec::new(),
            completed_tool_permission_audits: Vec::new(),
            tool_approval_requests: Vec::new(),
            pause: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum StandardPhase {
    InputIngest,
    ContextPrepare,
    ContextCompact,
    ModelRequestPrepare,
    ModelStream,
    AssistantCommit,
    ToolCallResolve,
    ToolExecute,
    TurnDecision,
}

impl PhaseNode for StandardPhase {
    fn id(&self) -> &str {
        match self {
            Self::InputIngest => PHASE_INPUT_INGEST,
            Self::ContextPrepare => PHASE_CONTEXT_PREPARE,
            Self::ContextCompact => PHASE_CONTEXT_COMPACT,
            Self::ModelRequestPrepare => PHASE_MODEL_REQUEST_PREPARE,
            Self::ModelStream => PHASE_MODEL_STREAM,
            Self::AssistantCommit => PHASE_ASSISTANT_COMMIT,
            Self::ToolCallResolve => PHASE_TOOL_CALL_RESOLVE,
            Self::ToolExecute => PHASE_TOOL_EXECUTE,
            Self::TurnDecision => PHASE_TURN_DECISION,
        }
    }

    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async move {
            match self {
                Self::InputIngest => standard::input_ingest(context).await,
                Self::ContextPrepare => standard::context_prepare(context).await,
                Self::ContextCompact => standard::context_compact(context).await,
                Self::ModelRequestPrepare => standard::model_request_prepare(context).await,
                Self::ModelStream => standard::model_stream(context).await,
                Self::AssistantCommit => assistant::assistant_commit(context).await,
                Self::ToolCallResolve => standard::tool_call_resolve(context).await,
                Self::ToolExecute => tool::tool_execute(context).await,
                Self::TurnDecision => standard::turn_decision(context).await,
            }
        })
    }
}
