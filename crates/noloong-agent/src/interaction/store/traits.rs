use super::AgentSessionRecord;
use crate::interaction::{InteractionError, InteractionFuture};

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

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>>;
}

pub(in crate::interaction) fn duplicate_session_error(session_id: &str) -> InteractionError {
    InteractionError::invalid_params(format!("session already exists: {session_id}"))
}

pub(in crate::interaction) fn missing_session_error(session_id: &str) -> InteractionError {
    InteractionError::not_found(format!("session not found: {session_id}"))
}
