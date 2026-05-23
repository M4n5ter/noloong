use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ExtensionCapabilitySelector {
    ModelProvider { id: String },
    Tool { name: String },
    ContextProvider { id: String },
    PhaseNode { id: String },
    PhaseHook { id: String },
    ToolCallHook { id: String },
    CompactionSummarizer { id: String },
    ContextCompactor { id: String },
    HttpAuthProvider { id: String },
}

impl ExtensionCapabilitySelector {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ModelProvider { id }
            | Self::ContextProvider { id }
            | Self::PhaseNode { id }
            | Self::PhaseHook { id }
            | Self::ToolCallHook { id }
            | Self::CompactionSummarizer { id }
            | Self::ContextCompactor { id }
            | Self::HttpAuthProvider { id } => validate_non_empty("capability id", id),
            Self::Tool { name } => validate_non_empty("tool name", name),
        }
    }
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}
