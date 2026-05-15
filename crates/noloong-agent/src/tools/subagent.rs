use crate::{BuiltInToolName, Catalog, MessageKey};
use noloong_agent_core::{
    AgentCoreError, AgentMessage, AgentState, BoxFuture, CancellationToken, ContentBlock,
    MessageRole, Result, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::Arc;

use super::{json_tool_output, sequential_tool_spec};

pub const MIN_SUBAGENT_WAIT_TIMEOUT_MS: u64 = 1;
pub const DEFAULT_SUBAGENT_WAIT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_SUBAGENT_WAIT_TIMEOUT_MS: u64 = 600_000;

pub trait SubagentController: Send + Sync {
    fn spawn_subagent<'a>(
        &'a self,
        request: SubagentSpawnRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentSummary>;

    fn wait_subagents<'a>(
        &'a self,
        session_ids: Vec<String>,
        timeout_ms: u64,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentWaitOutcome>;

    fn subagent_result<'a>(
        &'a self,
        session_id: String,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentResult>;

    fn list_subagents<'a>(
        &'a self,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<SubagentSummary>>;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSpawnRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSummary {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentResult {
    #[serde(flatten)]
    pub summary: SubagentSummary,
    pub settled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<SubagentFinalOutput>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentWaitOutcome {
    pub timed_out: bool,
    pub results: Vec<SubagentResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentFinalOutput {
    pub message: AgentMessage,
    pub final_text: String,
}

#[derive(Clone)]
pub struct SubagentSpawnTool {
    controller: Arc<dyn SubagentController>,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct SubagentWaitTool {
    controller: Arc<dyn SubagentController>,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct SubagentResultTool {
    controller: Arc<dyn SubagentController>,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct SubagentListTool {
    controller: Arc<dyn SubagentController>,
    catalog: Catalog,
}

impl SubagentSpawnTool {
    pub fn new(controller: Arc<dyn SubagentController>, catalog: Catalog) -> Self {
        Self {
            controller,
            catalog,
        }
    }
}

impl SubagentWaitTool {
    pub fn new(controller: Arc<dyn SubagentController>, catalog: Catalog) -> Self {
        Self {
            controller,
            catalog,
        }
    }
}

impl SubagentResultTool {
    pub fn new(controller: Arc<dyn SubagentController>, catalog: Catalog) -> Self {
        Self {
            controller,
            catalog,
        }
    }
}

impl SubagentListTool {
    pub fn new(controller: Arc<dyn SubagentController>, catalog: Catalog) -> Self {
        Self {
            controller,
            catalog,
        }
    }
}

impl ToolProvider for SubagentSpawnTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            BuiltInToolName::SubagentSpawn.as_str(),
            self.catalog.message(MessageKey::SubagentSpawnDescription),
            json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "role": {"type": "string"},
                    "prompt": {"type": "string"},
                    "metadata": {"type": "object"}
                }
            }),
            self.catalog
                .message(MessageKey::SubagentPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = serde_json::from_value::<SubagentSpawnInput>(request.arguments).map_err(
                |error| AgentCoreError::InvalidEffect(self.catalog.render_tool_input_error(error)),
            )?;
            let spawn_request = input.into_request()?;
            let summary = self
                .controller
                .spawn_subagent(spawn_request, cancellation)
                .await?;
            Ok(json_tool_output(json!(summary)))
        })
    }
}

impl ToolProvider for SubagentWaitTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            BuiltInToolName::SubagentWait.as_str(),
            self.catalog.message(MessageKey::SubagentWaitDescription),
            json!({
                "type": "object",
                "required": ["sessionIds"],
                "properties": {
                    "sessionIds": {
                        "type": "array",
                        "items": {"type": "string"},
                        "minItems": 1
                    },
                    "timeoutMs": {
                        "type": "integer",
                        "minimum": MIN_SUBAGENT_WAIT_TIMEOUT_MS,
                        "maximum": MAX_SUBAGENT_WAIT_TIMEOUT_MS
                    }
                }
            }),
            self.catalog
                .message(MessageKey::SubagentPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = serde_json::from_value::<SubagentWaitInput>(request.arguments).map_err(
                |error| AgentCoreError::InvalidEffect(self.catalog.render_tool_input_error(error)),
            )?;
            let timeout_ms = validate_timeout(input.timeout_ms)?;
            if input.session_ids.is_empty() {
                return Err(AgentCoreError::InvalidEffect(
                    "sessionIds must contain at least one subagent session id".into(),
                ));
            }
            let outcome = self
                .controller
                .wait_subagents(input.session_ids, timeout_ms, cancellation)
                .await?;
            Ok(json_tool_output(json!(outcome)))
        })
    }
}

impl ToolProvider for SubagentResultTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            BuiltInToolName::SubagentResult.as_str(),
            self.catalog.message(MessageKey::SubagentResultDescription),
            json!({
                "type": "object",
                "required": ["sessionId"],
                "properties": {
                    "sessionId": {"type": "string"}
                }
            }),
            self.catalog
                .message(MessageKey::SubagentPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = serde_json::from_value::<SubagentResultInput>(request.arguments).map_err(
                |error| AgentCoreError::InvalidEffect(self.catalog.render_tool_input_error(error)),
            )?;
            let result = self
                .controller
                .subagent_result(input.session_id, cancellation)
                .await?;
            Ok(json_tool_output(json!(result)))
        })
    }
}

impl ToolProvider for SubagentListTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            BuiltInToolName::SubagentList.as_str(),
            self.catalog.message(MessageKey::SubagentListDescription),
            json!({"type": "object", "properties": {}}),
            self.catalog
                .message(MessageKey::SubagentPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let subagents = self.controller.list_subagents(cancellation).await?;
            Ok(json_tool_output(json!({ "subagents": subagents })))
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentSpawnInput {
    role: Option<String>,
    prompt: String,
    #[serde(default)]
    metadata: Map<String, Value>,
}

impl SubagentSpawnInput {
    fn into_request(self) -> Result<SubagentSpawnRequest> {
        let prompt = self.prompt.trim();
        if prompt.is_empty() {
            return Err(AgentCoreError::InvalidEffect(
                "prompt must not be empty".into(),
            ));
        }
        let role = self.role.and_then(|role| {
            let role = role.trim();
            (!role.is_empty()).then(|| role.to_owned())
        });
        Ok(SubagentSpawnRequest {
            role,
            prompt: prompt.to_owned(),
            metadata: self.metadata,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentWaitInput {
    session_ids: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentResultInput {
    session_id: String,
}

pub fn final_assistant_output(state: &AgentState) -> Option<SubagentFinalOutput> {
    let message = state
        .messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant)?
        .clone();
    let final_text = assistant_text(&message);
    Some(SubagentFinalOutput {
        message,
        final_text,
    })
}

fn assistant_text(message: &AgentMessage) -> String {
    let mut text = String::new();
    for block in &message.content {
        if let ContentBlock::Text { text: block_text } = block {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(block_text);
        }
    }
    text
}

fn validate_timeout(timeout_ms: Option<u64>) -> Result<u64> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_SUBAGENT_WAIT_TIMEOUT_MS);
    if !(MIN_SUBAGENT_WAIT_TIMEOUT_MS..=MAX_SUBAGENT_WAIT_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(AgentCoreError::InvalidEffect(format!(
            "timeoutMs must be between {MIN_SUBAGENT_WAIT_TIMEOUT_MS} and {MAX_SUBAGENT_WAIT_TIMEOUT_MS}"
        )));
    }
    Ok(timeout_ms)
}

fn tool_spec(
    name: &str,
    description: &str,
    input_schema: Value,
    permission_description: &str,
) -> ToolSpec {
    sequential_tool_spec(
        name,
        description,
        input_schema,
        "agent.subagent",
        permission_description,
    )
}
