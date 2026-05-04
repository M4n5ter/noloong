use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type RunId = String;
pub type MessageId = String;
pub type ToolCallId = String;
pub type ToolApprovalId = String;
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
        thinking: ThinkingBlock,
    },
    Media {
        media: MediaBlock,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    File,
    Image,
    Audio,
    Video,
    Custom(String),
}

impl MediaKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::File => "file",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Custom(kind) => kind,
        }
    }
}

impl Serialize for MediaKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "file" => Self::File,
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => Self::Custom(kind),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MediaEncoding {
    #[default]
    Base64,
    Custom(String),
}

impl MediaEncoding {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Base64 => "base64",
            Self::Custom(encoding) => encoding,
        }
    }
}

impl Serialize for MediaEncoding {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaEncoding {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoding = String::deserialize(deserializer)?;
        Ok(match encoding.as_str() {
            "base64" => Self::Base64,
            _ => Self::Custom(encoding),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSource {
    Uri {
        uri: String,
    },
    Inline {
        data: String,
        encoding: MediaEncoding,
    },
    Provider {
        #[serde(rename = "providerId")]
        provider_id: String,
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncodedMediaData {
    pub data: String,
    pub encoding: MediaEncoding,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaBlock {
    pub kind: MediaKind,
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<EncodedMediaData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl MediaBlock {
    pub fn uri(kind: MediaKind, uri: impl Into<String>) -> Self {
        Self::new(kind, MediaSource::Uri { uri: uri.into() })
    }

    pub fn inline_base64(kind: MediaKind, data: impl Into<String>) -> Self {
        Self::new(
            kind,
            MediaSource::Inline {
                data: data.into(),
                encoding: MediaEncoding::Base64,
            },
        )
    }

    pub fn provider(
        kind: MediaKind,
        provider_id: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        Self::new(
            kind,
            MediaSource::Provider {
                provider_id: provider_id.into(),
                id: id.into(),
            },
        )
    }

    pub fn from_delta(delta: &MediaDelta) -> Option<Self> {
        let source = delta.source.clone().or_else(|| {
            delta.data_delta.as_ref().map(|data| MediaSource::Inline {
                data: data.clone(),
                encoding: MediaEncoding::Base64,
            })
        })?;
        let mut block = Self::new(delta.kind.clone(), source);
        if delta.source.is_some()
            && let Some(data_delta) = &delta.data_delta
        {
            block.append_encoded_data(data_delta, MediaEncoding::Base64);
        }
        block.mime_type.clone_from(&delta.mime_type);
        block.name.clone_from(&delta.name);
        block.replay_descriptor.clone_from(&delta.replay_descriptor);
        block.metadata.extend(delta.metadata.clone());
        Some(block)
    }

    pub fn apply_delta(&mut self, delta: &MediaDelta) {
        if let Some(source) = &delta.source {
            if !matches!(source, MediaSource::Inline { .. }) {
                self.move_inline_source_to_data();
            }
            self.source = source.clone();
        }
        if let Some(data_delta) = &delta.data_delta
            && !data_delta.is_empty()
        {
            if let MediaSource::Inline {
                data,
                encoding: MediaEncoding::Base64,
            } = &mut self.source
                && self.data.is_none()
            {
                data.push_str(data_delta);
            } else {
                self.append_encoded_data(data_delta, MediaEncoding::Base64);
            }
        }
        if let Some(mime_type) = &delta.mime_type {
            self.mime_type = Some(mime_type.clone());
        }
        if let Some(name) = &delta.name {
            self.name = Some(name.clone());
        }
        if let Some(replay_descriptor) = &delta.replay_descriptor {
            self.replay_descriptor = Some(replay_descriptor.clone());
        }
        self.metadata.extend(delta.metadata.clone());
    }

    fn move_inline_source_to_data(&mut self) {
        let MediaSource::Inline { data, encoding } = &self.source else {
            return;
        };
        if data.is_empty() {
            return;
        }
        let data = data.clone();
        let encoding = encoding.clone();
        self.append_encoded_data(&data, encoding);
    }

    fn append_encoded_data(&mut self, data_delta: &str, encoding: MediaEncoding) {
        if data_delta.is_empty() {
            return;
        }
        match &mut self.data {
            Some(data) if data.encoding == encoding => data.data.push_str(data_delta),
            _ => {
                self.data = Some(EncodedMediaData {
                    data: data_delta.into(),
                    encoding,
                });
            }
        }
    }

    fn new(kind: MediaKind, source: MediaSource) -> Self {
        Self {
            kind,
            source,
            data: None,
            mime_type: None,
            name: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaDelta {
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MediaSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub done: bool,
}

impl MediaDelta {
    pub fn from_inline_base64_delta(kind: MediaKind, data_delta: impl Into<String>) -> Self {
        Self {
            kind,
            data_delta: Some(data_delta.into()),
            source: None,
            mime_type: None,
            name: None,
            replay_descriptor: None,
            metadata: Map::new(),
            done: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data_delta.as_ref().is_none_or(String::is_empty)
            && self.source.is_none()
            && self.mime_type.is_none()
            && self.name.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
            && !self.done
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ThinkingKind {
    #[default]
    Raw,
    Summary,
    Redacted,
    Encrypted,
    Custom(String),
}

impl ThinkingKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Raw => "raw",
            Self::Summary => "summary",
            Self::Redacted => "redacted",
            Self::Encrypted => "encrypted",
            Self::Custom(kind) => kind,
        }
    }
}

impl Serialize for ThinkingKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThinkingKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "raw" => Self::Raw,
            "summary" => Self::Summary,
            "redacted" => Self::Redacted,
            "encrypted" => Self::Encrypted,
            _ => Self::Custom(kind),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingBlock {
    #[serde(default)]
    pub kind: ThinkingKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ThinkingBlock {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Raw,
            text: Some(text.into()),
            raw: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn from_delta(delta: &ThinkingDelta) -> Self {
        let mut block = Self {
            kind: delta.kind.clone(),
            text: None,
            raw: None,
            replay_descriptor: None,
            metadata: Map::new(),
        };
        block.apply_delta(delta);
        block
    }

    pub fn apply_delta(&mut self, delta: &ThinkingDelta) {
        if let Some(text_delta) = &delta.text_delta
            && !text_delta.is_empty()
        {
            self.text
                .get_or_insert_with(String::new)
                .push_str(text_delta);
        }
        if let Some(raw_snapshot) = &delta.raw_snapshot {
            self.raw = Some(raw_snapshot.clone());
        }
        if let Some(replay_descriptor) = &delta.replay_descriptor {
            self.replay_descriptor = Some(replay_descriptor.clone());
        }
        self.metadata.extend(delta.metadata.clone());
    }

    pub fn is_empty(&self) -> bool {
        self.text.as_ref().is_none_or(String::is_empty)
            && self.raw.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingDelta {
    #[serde(default)]
    pub kind: ThinkingKind,
    #[serde(default, alias = "text", skip_serializing_if = "Option::is_none")]
    pub text_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_snapshot: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ThinkingDelta {
    pub fn from_text(text_delta: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Raw,
            text_delta: Some(text_delta.into()),
            raw_snapshot: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn from_summary(text_delta: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Summary,
            text_delta: Some(text_delta.into()),
            raw_snapshot: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn with_raw(mut self, raw_snapshot: Value) -> Self {
        self.raw_snapshot = Some(raw_snapshot);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.text_delta.as_ref().is_none_or(String::is_empty)
            && self.raw_snapshot.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
    }
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
    PhaseHook { id: String },
    ToolCallHook { id: String },
    CompactionSummarizer { id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    pub name: String,
    pub version: String,
}
