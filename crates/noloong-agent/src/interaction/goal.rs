use super::store::current_unix_ms;
use noloong_agent_core::AgentMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const GOAL_AUDIT_SOURCE_TYPE: &str = "goal_audit";
pub const GOAL_AUDIT_REASON_TURN_END: &str = "turn_end";
pub const GOAL_AUDIT_REASON_TOOL_UPDATE: &str = "tool_update";
pub const GOAL_AUDIT_MESSAGE_ID_PREFIX: &str = "goal-audit";
pub const GOAL_UPDATE_STATUS_ERROR: &str =
    "goal update status must be pursuing, achieved, unmet, or budget_limited";
pub const GOAL_UPDATE_ALLOWED_STATUS_VALUES: &[&str] =
    &["pursuing", "achieved", "unmet", "budget_limited"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoalRecord {
    pub goal_id: String,
    pub session_id: String,
    pub objective: String,
    pub status: GoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_audit: Option<GoalAuditRecord>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl GoalRecord {
    pub fn new(session_id: impl Into<String>, objective: impl Into<String>) -> Self {
        let now = current_unix_ms();
        let session_id = session_id.into();
        Self {
            goal_id: format!("goal-{session_id}-{now}"),
            session_id,
            objective: objective.into(),
            status: GoalStatus::Pursuing,
            token_budget: None,
            last_audit: None,
            metadata: Map::new(),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn is_pursuing(&self) -> bool {
        self.status == GoalStatus::Pursuing
    }

    pub fn mark_updated(&mut self) {
        self.updated_at_ms = current_unix_ms();
    }

    pub fn audit_pending(&self) -> bool {
        self.last_audit.as_ref().is_some_and(|audit| audit.pending)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Pursuing,
    Paused,
    Achieved,
    Unmet,
    BudgetLimited,
    Cleared,
}

impl GoalStatus {
    pub fn is_goal_update_allowed(&self) -> bool {
        matches!(
            self,
            Self::Pursuing | Self::Achieved | Self::Unmet | Self::BudgetLimited
        )
    }
}

pub fn trim_non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoalAuditRecord {
    pub reason: String,
    pub run_id: String,
    pub turn_id: u64,
    pub pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    pub audited_at_ms: u64,
}

pub fn goal_audit_message(goal: &GoalRecord, run_id: &str, turn_id: u64) -> AgentMessage {
    let mut message = AgentMessage::user(
        format!("{GOAL_AUDIT_MESSAGE_ID_PREFIX}-{run_id}-{turn_id}"),
        format!(
            "Goal audit request.\n\nCurrent goal: {}\n\nReview the latest turn against this goal. If the goal status changed, call `agent.goal.update` with the new status, summary, and evidence. Do not mark the goal complete using prose alone.",
            goal.objective
        ),
    );
    message.metadata.insert(
        "source".into(),
        json!({
            "type": GOAL_AUDIT_SOURCE_TYPE,
            "goalId": goal.goal_id,
            "sessionId": goal.session_id,
            "auditReason": GOAL_AUDIT_REASON_TURN_END,
        }),
    );
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_record_serde_round_trips() {
        let mut goal = GoalRecord::new("session-1", "finish the work");
        goal.token_budget = Some(100);
        goal.last_audit = Some(GoalAuditRecord {
            reason: GOAL_AUDIT_REASON_TURN_END.into(),
            run_id: "run-1".into(),
            turn_id: 2,
            pending: true,
            summary: Some("checking".into()),
            evidence: None,
            audited_at_ms: 123,
        });

        let value = serde_json::to_value(&goal).unwrap();
        assert_eq!(value["status"], "pursuing");
        assert_eq!(value["tokenBudget"], 100);

        let decoded: GoalRecord = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, goal);
    }

    #[test]
    fn goal_audit_message_marks_source() {
        let goal = GoalRecord::new("session-1", "ship");
        let message = goal_audit_message(&goal, "run-1", 3);
        assert_eq!(message.metadata["source"]["type"], GOAL_AUDIT_SOURCE_TYPE);
        assert_eq!(message.metadata["source"]["auditReason"], "turn_end");
    }
}
