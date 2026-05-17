use crate::AgentManifest;
use noloong_agent_core::{
    AgentMessage, AgentState, QueueMode, QueuedAgentMessage, QueuedMessageIntent,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const AGENT_SESSION_RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub session_id: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub run_id_prefix: String,
    pub manifest: AgentManifest,
    #[serde(default)]
    pub state: AgentState,
    #[serde(default)]
    pub queues: AgentSessionQueueSnapshot,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AgentSessionRecord {
    pub fn validate_schema_version(&self) -> Result<(), String> {
        if self.schema_version == AGENT_SESSION_RECORD_SCHEMA_VERSION {
            return Ok(());
        }
        Err(format!(
            "unsupported agent session record schema version: {}",
            self.schema_version
        ))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionQueueSnapshot {
    #[serde(default)]
    pub steering: AgentSessionQueueState,
    #[serde(default)]
    pub follow_up: AgentSessionQueueState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionQueueState {
    #[serde(default = "default_queue_mode")]
    pub mode: QueueMode,
    #[serde(default)]
    pub messages: Vec<AgentSessionQueuedMessage>,
}

impl Default for AgentSessionQueueState {
    fn default() -> Self {
        Self {
            mode: default_queue_mode(),
            messages: Vec::new(),
        }
    }
}

impl AgentSessionQueueState {
    pub fn from_core(mode: QueueMode, messages: Vec<QueuedAgentMessage>) -> Self {
        Self {
            mode,
            messages: messages
                .into_iter()
                .map(AgentSessionQueuedMessage::from_core)
                .collect(),
        }
    }

    pub fn into_core_messages(self) -> Vec<QueuedAgentMessage> {
        self.messages
            .into_iter()
            .map(AgentSessionQueuedMessage::into_core)
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionQueuedMessage {
    pub message: AgentMessage,
    #[serde(default)]
    pub intent: AgentSessionQueuedMessageIntent,
}

impl AgentSessionQueuedMessage {
    pub fn from_core(message: QueuedAgentMessage) -> Self {
        Self {
            message: message.message,
            intent: AgentSessionQueuedMessageIntent::from(message.intent),
        }
    }

    pub fn into_core(self) -> QueuedAgentMessage {
        match self.intent {
            AgentSessionQueuedMessageIntent::Observation => {
                QueuedAgentMessage::observation(self.message)
            }
            AgentSessionQueuedMessageIntent::UserInput => {
                QueuedAgentMessage::user_input(self.message)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionQueuedMessageIntent {
    #[default]
    Observation,
    UserInput,
}

impl From<QueuedMessageIntent> for AgentSessionQueuedMessageIntent {
    fn from(value: QueuedMessageIntent) -> Self {
        match value {
            QueuedMessageIntent::Observation => Self::Observation,
            QueuedMessageIntent::UserInput => Self::UserInput,
        }
    }
}

pub fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn default_schema_version() -> u32 {
    AGENT_SESSION_RECORD_SCHEMA_VERSION
}

fn default_queue_mode() -> QueueMode {
    QueueMode::OneAtATime
}
