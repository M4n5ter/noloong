use crate::{
    BuiltInToolName, Catalog, GoalRecord, GoalStatus, MessageKey,
    interaction::{
        GOAL_AUDIT_REASON_TOOL_UPDATE, GOAL_UPDATE_ALLOWED_STATUS_VALUES, GOAL_UPDATE_STATUS_ERROR,
        GoalAuditRecord, current_unix_ms, trim_non_empty,
    },
};
use noloong_agent_core::{
    AgentCoreError, BoxFuture, CancellationToken, Result, ToolOutput, ToolProvider, ToolRequest,
    ToolSpec,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::{json_tool_output, sequential_tool_spec};

pub const GOAL_PERMISSION_CAPABILITY: &str = "agent.goal";

pub trait GoalController: Send + Sync {
    fn update_goal<'a>(
        &'a self,
        request: GoalUpdateRequest,
        run_id: String,
        turn_id: u64,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, GoalRecord>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalUpdateRequest {
    pub status: GoalStatus,
    pub summary: Option<String>,
    pub evidence: Option<String>,
}

#[derive(Clone)]
pub struct GoalUpdateTool {
    controller: Arc<dyn GoalController>,
    catalog: Catalog,
}

impl GoalUpdateTool {
    pub fn new(controller: Arc<dyn GoalController>, catalog: Catalog) -> Self {
        Self {
            controller,
            catalog,
        }
    }
}

impl ToolProvider for GoalUpdateTool {
    fn spec(&self) -> ToolSpec {
        sequential_tool_spec(
            BuiltInToolName::GoalUpdate.as_str(),
            self.catalog.message(MessageKey::GoalUpdateDescription),
            json!({
                "type": "object",
                "required": ["status"],
                "additionalProperties": false,
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": GOAL_UPDATE_ALLOWED_STATUS_VALUES
                    },
                    "summary": {"type": "string"},
                    "evidence": {"type": "string"}
                }
            }),
            GOAL_PERMISSION_CAPABILITY,
            self.catalog.message(MessageKey::GoalPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input =
                serde_json::from_value::<GoalUpdateInput>(request.arguments).map_err(|error| {
                    AgentCoreError::InvalidEffect(self.catalog.render_tool_input_error(error))
                })?;
            let goal = self
                .controller
                .update_goal(
                    input.into_request()?,
                    request.run_id,
                    request.turn_id,
                    cancellation,
                )
                .await?;
            Ok(json_tool_output(json!(goal)))
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoalUpdateInput {
    status: GoalStatus,
    summary: Option<String>,
    evidence: Option<String>,
}

impl GoalUpdateInput {
    fn into_request(self) -> Result<GoalUpdateRequest> {
        if !self.status.is_goal_update_allowed() {
            return Err(AgentCoreError::InvalidEffect(
                GOAL_UPDATE_STATUS_ERROR.into(),
            ));
        }
        Ok(GoalUpdateRequest {
            status: self.status,
            summary: self.summary.and_then(trim_non_empty),
            evidence: self.evidence.and_then(trim_non_empty),
        })
    }
}

pub fn update_goal_audit(
    mut goal: GoalRecord,
    request: &GoalUpdateRequest,
    run_id: String,
    turn_id: u64,
) -> GoalRecord {
    goal.status = request.status.clone();
    goal.last_audit = Some(GoalAuditRecord {
        reason: GOAL_AUDIT_REASON_TOOL_UPDATE.into(),
        run_id,
        turn_id,
        pending: false,
        summary: request.summary.clone(),
        evidence: request.evidence.clone(),
        audited_at_ms: current_unix_ms(),
    });
    goal.mark_updated();
    goal
}
