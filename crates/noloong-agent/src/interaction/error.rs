use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Display, Formatter};

pub const INTERACTION_ERROR_METHOD_NOT_FOUND: i64 = -32601;
pub const INTERACTION_ERROR_INVALID_PARAMS: i64 = -32602;
pub const INTERACTION_ERROR_INTERNAL: i64 = -32603;
pub const INTERACTION_ERROR_UNAUTHORIZED: i64 = -32070;
pub const INTERACTION_ERROR_BUSY: i64 = -32071;
pub const INTERACTION_ERROR_NOT_FOUND: i64 = -32072;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl InteractionError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            INTERACTION_ERROR_METHOD_NOT_FOUND,
            format!("unknown method: {method}"),
        )
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INTERACTION_ERROR_INVALID_PARAMS, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(INTERACTION_ERROR_INTERNAL, message)
    }

    pub fn unauthorized(
        method: &str,
        capability: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let capability = capability.into();
        Self::new(INTERACTION_ERROR_UNAUTHORIZED, message).with_data(serde_json::json!({
            "method": method,
            "requiredCapability": capability,
        }))
    }

    pub fn busy(message: impl Into<String>) -> Self {
        Self::new(INTERACTION_ERROR_BUSY, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(INTERACTION_ERROR_NOT_FOUND, message)
    }
}

impl Display for InteractionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for InteractionError {}

impl From<noloong_agent_core::AgentCoreError> for InteractionError {
    fn from(error: noloong_agent_core::AgentCoreError) -> Self {
        match error {
            noloong_agent_core::AgentCoreError::Phase(message) => Self::busy(message),
            error => Self::internal(error.to_string()),
        }
    }
}

impl From<crate::ProcessError> for InteractionError {
    fn from(error: crate::ProcessError) -> Self {
        match error {
            crate::ProcessError::Invalid(message) => Self::invalid_params(message),
            crate::ProcessError::UnknownJob(job_id) => {
                Self::not_found(format!("process job not found: {job_id}"))
            }
            crate::ProcessError::Spawn(message) | crate::ProcessError::Io(message) => {
                Self::internal(message)
            }
        }
    }
}

impl From<serde_json::Error> for InteractionError {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid_params(error.to_string())
    }
}
