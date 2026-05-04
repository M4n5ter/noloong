use crate::{
    AgentEffect, AgentMessage, ContentBlock, ExtensionCapability, ExtensionManifest, ModelRequest,
    ModelStreamEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(super) struct JsonRpcRequest<'a> {
    pub(super) jsonrpc: &'a str,
    pub(super) id: u64,
    pub(super) method: &'a str,
    pub(super) params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InitializeResult {
    pub(super) manifest: ExtensionManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CapabilitiesResult {
    pub(super) capabilities: Vec<ExtensionCapability>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StreamResult {
    pub(super) stream_id: Option<String>,
    #[serde(default)]
    pub(super) events: Vec<ModelStreamEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContextResult {
    #[serde(default)]
    pub(super) effects: Vec<AgentEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PhaseHookOutput {
    #[serde(default)]
    pub(super) model_request: Option<ModelRequest>,
    #[serde(default)]
    pub(super) model_events: Option<Vec<ModelStreamEvent>>,
    #[serde(default)]
    pub(super) assistant_message: Option<AgentMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeforeToolHookOutput {
    #[serde(default)]
    pub(super) decision: Option<crate::ToolPermissionDecision>,
    #[serde(default)]
    pub(super) approval: Option<crate::ToolApprovalRequestSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AfterToolHookOutput {
    #[serde(default)]
    pub(super) content: Option<Vec<ContentBlock>>,
    #[serde(default)]
    pub(super) details: Option<Value>,
    #[serde(default)]
    pub(super) is_error: Option<bool>,
}
