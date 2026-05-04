use super::ToolSpec;
use serde::{Deserialize, Serialize};

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
