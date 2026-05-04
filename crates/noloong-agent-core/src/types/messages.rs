use super::{MediaBlock, MessageId, ThinkingBlock, ToolCall, ToolCallId, ToolOutput};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

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
