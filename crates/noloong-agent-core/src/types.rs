use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type RunId = String;
pub type MessageId = String;
pub type ToolCallId = String;
pub type TurnId = u64;
pub type EventSequence = u64;

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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ContextPatch {
    Set { key: String, value: Value },
    Remove { key: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentState {
    pub run_id: Option<RunId>,
    pub status: RunStatus,
    pub messages: Vec<AgentMessage>,
    pub context: BTreeMap<String, Value>,
    pub available_tools: BTreeMap<String, ToolSpec>,
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
    All,
    #[default]
    OneAtATime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: MessageId,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl AgentMessage {
    pub fn user(id: impl Into<MessageId>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            metadata: Map::new(),
        }
    }

    pub fn assistant(id: impl Into<MessageId>, content: Vec<ContentBlock>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::Assistant,
            content,
            metadata: Map::new(),
        }
    }

    pub fn tool_result(
        id: impl Into<MessageId>,
        tool_call_id: impl Into<ToolCallId>,
        tool_name: impl Into<String>,
        output: ToolOutput,
    ) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::ToolResult,
            content: vec![ContentBlock::ToolResult {
                tool_call_id: tool_call_id.into(),
                tool_name: tool_name.into(),
                content: output.content,
                is_error: output.is_error,
            }],
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    ToolResult,
    System,
    Custom(String),
}

impl MessageRole {
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolResult => "tool_result",
            Self::System => "system",
            Self::Custom(role) => role,
        }
    }
}

impl Serialize for MessageRole {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MessageRole {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let role = String::deserialize(deserializer)?;
        Ok(match role.as_str() {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "tool_result" => Self::ToolResult,
            "system" => Self::System,
            _ => Self::Custom(role),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Thinking {
        text: String,
    },
    Text {
        text: String,
    },
    Json {
        value: Value,
    },
    ToolCall {
        tool_call: ToolCall,
    },
    ToolResult {
        tool_call_id: ToolCallId,
        tool_name: String,
        content: Vec<ContentBlock>,
        is_error: bool,
    },
}

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
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStreamEvent {
    Started { stream_id: String },
    ThinkingDelta { text: String },
    TextDelta { text: String },
    ToolCall { tool_call: ToolCall },
    Finished { stop_reason: StopReason },
    Failed { error: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeToolCallContext {
    pub run_id: String,
    pub turn_id: u64,
    pub tool_call: ToolCall,
    pub state: AgentState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeToolCallResult {
    #[serde(default)]
    pub block: bool,
    #[serde(default)]
    pub reason: Option<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionCapability {
    ModelProvider { id: String },
    Tool { spec: ToolSpec },
    ContextProvider { id: String },
    PhaseNode { id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    pub name: String,
    pub version: String,
}
