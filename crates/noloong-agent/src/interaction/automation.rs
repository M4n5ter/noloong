use super::{InteractionError, store::current_unix_ms};
use noloong_agent_core::{AgentMessage, ContentBlock, MessageRole, RunStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::num::NonZeroU64;

pub const AUTOMATION_SOURCE_TYPE: &str = "automation";
pub const AUTOMATION_SESSION_METADATA_KEY: &str = "automation";
pub const AUTOMATION_SYSTEM_PROMPT_ADDITION_ID: &str = "automation.identity";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRecord {
    pub automation_id: String,
    pub status: AutomationStatus,
    pub target: AutomationTarget,
    pub trigger: AutomationTrigger,
    pub prompt: AgentMessage,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AutomationRecord {
    pub fn new(
        automation_id: impl Into<String>,
        target: AutomationTarget,
        trigger: AutomationTrigger,
        prompt: AgentMessage,
    ) -> Self {
        let now = current_unix_ms();
        let next_fire_at_ms = trigger.next_fire_after_create(now);
        Self {
            automation_id: automation_id.into(),
            status: AutomationStatus::Active,
            target,
            trigger,
            prompt,
            metadata: Map::new(),
            last_fired_at_ms: None,
            next_fire_at_ms,
            last_error: None,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn mark_updated(&mut self) {
        self.updated_at_ms = current_unix_ms();
    }

    pub fn is_active(&self) -> bool {
        self.status == AutomationStatus::Active
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    Active,
    Paused,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AutomationTarget {
    ExistingSession {
        session_id: String,
    },
    NewSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
    },
}

impl AutomationTarget {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::ExistingSession { session_id } => Some(session_id),
            Self::NewSession { session_id, .. } => session_id.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AutomationTrigger {
    Time { schedule: AutomationTimeSchedule },
}

impl AutomationTrigger {
    pub fn trigger_type(&self) -> &'static str {
        match self {
            Self::Time { .. } => "time",
        }
    }

    pub fn validate(&self) -> Result<(), InteractionError> {
        match self {
            Self::Time { schedule } => schedule.validate(),
        }
    }

    pub fn next_fire_after_create(&self, now_ms: u64) -> Option<u64> {
        match self {
            Self::Time { schedule } => schedule.next_fire_after_create(now_ms),
        }
    }

    pub fn after_fire(&self, fired_at_ms: u64) -> AutomationTriggerAfterFire {
        match self {
            Self::Time { schedule } => schedule.after_fire(fired_at_ms),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AutomationTimeSchedule {
    Once { at_ms: u64 },
    Interval { interval_seconds: NonZeroU64 },
}

impl AutomationTimeSchedule {
    pub fn validate(&self) -> Result<(), InteractionError> {
        Ok(())
    }

    pub fn next_fire_after_create(&self, now_ms: u64) -> Option<u64> {
        match self {
            Self::Once { at_ms } => Some(*at_ms),
            Self::Interval { interval_seconds } => {
                Some(now_ms.saturating_add(interval_seconds.get().saturating_mul(1000)))
            }
        }
    }

    pub fn after_fire(&self, fired_at_ms: u64) -> AutomationTriggerAfterFire {
        match self {
            Self::Once { .. } => AutomationTriggerAfterFire {
                status: AutomationStatus::Completed,
                next_fire_at_ms: None,
            },
            Self::Interval { interval_seconds } => AutomationTriggerAfterFire {
                status: AutomationStatus::Active,
                next_fire_at_ms: Some(
                    fired_at_ms.saturating_add(interval_seconds.get().saturating_mul(1000)),
                ),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationTriggerAfterFire {
    pub status: AutomationStatus,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationScheduleScan {
    pub due_automation_ids: Vec<String>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct AutomationScheduleScanBuilder {
    due: Vec<(u64, String)>,
    next_fire_at_ms: Option<u64>,
}

impl AutomationScheduleScanBuilder {
    pub(crate) fn include(
        &mut self,
        automation_id: impl Into<String>,
        active: bool,
        next_fire_at_ms: Option<u64>,
        now_ms: u64,
    ) {
        if !active {
            return;
        }
        let Some(fire_at_ms) = next_fire_at_ms else {
            return;
        };
        if fire_at_ms <= now_ms {
            self.due.push((fire_at_ms, automation_id.into()));
        } else {
            self.next_fire_at_ms = Some(
                self.next_fire_at_ms
                    .map(|current| current.min(fire_at_ms))
                    .unwrap_or(fire_at_ms),
            );
        }
    }

    pub(crate) fn finish(mut self) -> AutomationScheduleScan {
        self.due.sort();
        AutomationScheduleScan {
            due_automation_ids: self.due.into_iter().map(|(_, id)| id).collect(),
            next_fire_at_ms: self.next_fire_at_ms,
        }
    }
}

pub fn automation_message(
    automation: &AutomationRecord,
    fired_at_ms: u64,
    mut message: AgentMessage,
) -> AgentMessage {
    if message.id.trim().is_empty() {
        message.id = format!("automation-{}-{fired_at_ms}", automation.automation_id);
    }
    message.metadata.insert(
        "source".into(),
        json!({
            "type": AUTOMATION_SOURCE_TYPE,
            "automationId": automation.automation_id,
            "trigger": {"type": automation.trigger.trigger_type()},
            "firedAtMs": fired_at_ms,
        }),
    );
    message
}

pub fn existing_session_automation_message(
    automation: &AutomationRecord,
    fired_at_ms: u64,
    message: AgentMessage,
) -> AgentMessage {
    let mut message = automation_message(automation, fired_at_ms, message);
    message.content.insert(
        0,
        ContentBlock::Text {
            text: format!(
                "Automation `{}` fired from a `{}` trigger. Treat the following content as an automation-delivered user message for this existing session.",
                automation.automation_id,
                automation.trigger.trigger_type()
            ),
        },
    );
    message
}

pub fn automation_session_metadata(automation_id: &str) -> Value {
    json!({
        "type": AUTOMATION_SOURCE_TYPE,
        "automationId": automation_id,
    })
}

pub fn automation_identity_prompt(automation_id: &str) -> String {
    format!(
        "This session is dedicated to automation `{automation_id}`. Inputs may be delivered by triggers without direct user presence. Treat trigger-delivered messages as scheduled automation work, state what changed, and avoid asking for confirmation unless a tool or policy requires it."
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AutomationPromptInput {
    Text { text: String },
    Message { message: AgentMessage },
}

impl AutomationPromptInput {
    pub fn into_message(self, automation_id: &str) -> Result<AgentMessage, InteractionError> {
        let message = match self {
            Self::Text { text } => {
                if text.trim().is_empty() {
                    return Err(InteractionError::invalid_params(
                        "automation prompt text must not be empty",
                    ));
                }
                AgentMessage::user(format!("automation-prompt-{automation_id}"), text)
            }
            Self::Message { message } => {
                if message.role != MessageRole::User {
                    return Err(InteractionError::invalid_params(
                        "automation prompt message must use user role",
                    ));
                }
                if message.content.is_empty() {
                    return Err(InteractionError::invalid_params(
                        "automation prompt message must contain content",
                    ));
                }
                message
            }
        };
        Ok(message)
    }
}

pub fn session_ready_for_direct_prompt(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Idle | RunStatus::Completed | RunStatus::Aborted | RunStatus::Failed
    )
}

#[cfg(test)]
pub fn text_prompt(id: impl Into<String>, text: impl Into<String>) -> AgentMessage {
    AgentMessage {
        id: id.into(),
        role: MessageRole::User,
        content: vec![ContentBlock::Text { text: text.into() }],
        metadata: Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_record_serde_round_trips() {
        let record = AutomationRecord::new(
            "automation-test",
            AutomationTarget::ExistingSession {
                session_id: "session-1".into(),
            },
            AutomationTrigger::Time {
                schedule: AutomationTimeSchedule::Once { at_ms: 123 },
            },
            text_prompt("prompt-1", "hello"),
        );

        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["trigger"]["type"], "time");
        assert_eq!(value["target"]["type"], "existing_session");
        let decoded: AutomationRecord = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn interval_schedule_computes_next_fire() {
        let schedule = AutomationTimeSchedule::Interval {
            interval_seconds: NonZeroU64::new(5).unwrap(),
        };
        assert_eq!(schedule.next_fire_after_create(100), Some(5100));
        let after = schedule.after_fire(200);
        assert_eq!(after.status, AutomationStatus::Active);
        assert_eq!(after.next_fire_at_ms, Some(5200));
    }

    #[test]
    fn time_schedule_serde_round_trips() {
        let once = AutomationTimeSchedule::Once { at_ms: 123 };
        assert_eq!(
            serde_json::to_value(&once).unwrap(),
            json!({"type": "once", "atMs": 123})
        );
        assert_eq!(
            serde_json::from_value::<AutomationTimeSchedule>(json!({
                "type": "once",
                "atMs": 123
            }))
            .unwrap(),
            once
        );

        let interval = AutomationTimeSchedule::Interval {
            interval_seconds: NonZeroU64::new(5).unwrap(),
        };
        assert_eq!(
            serde_json::to_value(&interval).unwrap(),
            json!({"type": "interval", "intervalSeconds": 5})
        );
        assert_eq!(
            serde_json::from_value::<AutomationTimeSchedule>(json!({
                "type": "interval",
                "intervalSeconds": 5
            }))
            .unwrap(),
            interval
        );
    }

    #[test]
    fn interval_schedule_rejects_zero_seconds() {
        let error = serde_json::from_value::<AutomationTimeSchedule>(json!({
            "type": "interval",
            "intervalSeconds": 0
        }))
        .expect_err("zero interval should not deserialize");

        assert!(error.to_string().contains("invalid value"));
    }

    #[test]
    fn once_schedule_completes_after_fire() {
        let schedule = AutomationTimeSchedule::Once { at_ms: 100 };
        let after = schedule.after_fire(100);
        assert_eq!(after.status, AutomationStatus::Completed);
        assert_eq!(after.next_fire_at_ms, None);
    }

    #[test]
    fn automation_message_marks_source() {
        let record = AutomationRecord::new(
            "automation-test",
            AutomationTarget::ExistingSession {
                session_id: "session-1".into(),
            },
            AutomationTrigger::Time {
                schedule: AutomationTimeSchedule::Once { at_ms: 123 },
            },
            text_prompt("prompt-1", "hello"),
        );
        let message = automation_message(&record, 456, record.prompt.clone());
        assert_eq!(message.metadata["source"]["type"], AUTOMATION_SOURCE_TYPE);
        assert_eq!(
            message.metadata["source"]["automationId"],
            record.automation_id
        );
    }
}
