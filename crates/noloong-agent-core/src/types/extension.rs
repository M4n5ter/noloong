use super::ToolSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    ContextCompactor { id: String },
    HttpAuthProvider { id: String },
}

impl ExtensionCapability {
    pub fn selector(&self) -> ExtensionCapabilitySelector {
        match self {
            Self::ModelProvider { id } => {
                ExtensionCapabilitySelector::ModelProvider { id: id.clone() }
            }
            Self::Tool { spec } => ExtensionCapabilitySelector::Tool {
                name: spec.name.clone(),
            },
            Self::ContextProvider { id } => {
                ExtensionCapabilitySelector::ContextProvider { id: id.clone() }
            }
            Self::PhaseNode { id } => ExtensionCapabilitySelector::PhaseNode { id: id.clone() },
            Self::PhaseHook { id } => ExtensionCapabilitySelector::PhaseHook { id: id.clone() },
            Self::ToolCallHook { id } => {
                ExtensionCapabilitySelector::ToolCallHook { id: id.clone() }
            }
            Self::CompactionSummarizer { id } => {
                ExtensionCapabilitySelector::CompactionSummarizer { id: id.clone() }
            }
            Self::ContextCompactor { id } => {
                ExtensionCapabilitySelector::ContextCompactor { id: id.clone() }
            }
            Self::HttpAuthProvider { id } => {
                ExtensionCapabilitySelector::HttpAuthProvider { id: id.clone() }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
    pub fn matches(&self, capability: &ExtensionCapability) -> bool {
        self == &capability.selector()
    }

    pub fn validate_set(selectors: &BTreeSet<Self>) -> Result<(), String> {
        for selector in selectors {
            selector.validate()?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ModelProvider { id }
            | Self::ContextProvider { id }
            | Self::PhaseNode { id }
            | Self::PhaseHook { id }
            | Self::ToolCallHook { id }
            | Self::CompactionSummarizer { id }
            | Self::ContextCompactor { id }
            | Self::HttpAuthProvider { id } => validate_non_empty_identifier(id),
            Self::Tool { name } => validate_non_empty_identifier(name),
        }
    }
}

fn validate_non_empty_identifier(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("extension capability selector id must not be empty".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    pub name: String,
    pub version: String,
}
