use super::{AgentSessionRecord, AutomationRecord, AutomationScheduleScan, GoalRecord};
use crate::interaction::{
    AgentSessionListFilter, InteractionError, InteractionFuture, InteractionSessionStatus,
};
use serde_json::Value;

pub trait AgentSessionRegistryStore: Send + Sync {
    /// Inserts a new session record.
    ///
    /// Implementations should report a duplicate session id when they can detect it. The registry
    /// serializes create-session calls by id before calling this method, so backends without atomic
    /// create-if-absent support can rely on that caller-side reservation.
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()>;

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()>;

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()>;

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>>;

    fn list<'a>(
        &'a self,
        filter: &'a AgentSessionListFilter,
    ) -> InteractionFuture<'a, Vec<AgentSessionRecord>>;

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()>;

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>>;

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>>;

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()>;

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()>;

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()>;

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>>;

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>>;

    fn scan_automation_schedule<'a>(
        &'a self,
        now_ms: u64,
    ) -> InteractionFuture<'a, AutomationScheduleScan>;

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()>;
}

pub(in crate::interaction) fn duplicate_session_error(session_id: &str) -> InteractionError {
    InteractionError::invalid_params(format!("session already exists: {session_id}"))
}

pub(in crate::interaction) fn missing_session_error(session_id: &str) -> InteractionError {
    InteractionError::not_found(format!("session not found: {session_id}"))
}

pub(in crate::interaction) fn duplicate_automation_error(automation_id: &str) -> InteractionError {
    InteractionError::invalid_params(format!("automation already exists: {automation_id}"))
}

pub(in crate::interaction) fn missing_automation_error(automation_id: &str) -> InteractionError {
    InteractionError::not_found(format!("automation not found: {automation_id}"))
}

pub(in crate::interaction) fn record_matches_session_list_filter(
    record: &AgentSessionRecord,
    filter: &AgentSessionListFilter,
) -> bool {
    if filter
        .parent_session_id
        .as_ref()
        .is_some_and(|parent| record.parent_session_id.as_ref() != Some(parent))
    {
        return false;
    }
    if filter
        .profile_id
        .as_ref()
        .is_some_and(|profile| &record.profile_id != profile)
    {
        return false;
    }
    if filter.status.as_ref().is_some_and(|status| {
        *status != InteractionSessionStatus::from(record.state.status.clone())
    }) {
        return false;
    }
    filter
        .metadata_equals
        .iter()
        .all(|(key, expected)| record.metadata.get(key) == Some(expected))
}

pub(in crate::interaction) fn session_metadata_filter_value_supported(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

#[cfg(any(
    feature = "registry-store-object",
    feature = "registry-store-sqlite",
    feature = "registry-store-postgres"
))]
pub(in crate::interaction) fn session_metadata_index_value(value: &Value) -> Option<String> {
    session_metadata_filter_value_supported(value).then(|| value.to_string())
}
