use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, AgentCoreError>;

#[derive(Debug)]
pub enum AgentCoreError {
    Aborted,
    EventSink(String),
    InvalidEffect(String),
    MissingModelProvider(String),
    MissingTool(String),
    Phase(String),
    Provider(String),
    JsonRpc(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Join(tokio::task::JoinError),
}

impl Display for AgentCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aborted => write!(f, "run aborted"),
            Self::EventSink(message) => write!(f, "event sink failed: {message}"),
            Self::InvalidEffect(message) => write!(f, "invalid effect: {message}"),
            Self::MissingModelProvider(id) => write!(f, "model provider not found: {id}"),
            Self::MissingTool(name) => write!(f, "tool not found: {name}"),
            Self::Phase(message) => write!(f, "phase failed: {message}"),
            Self::Provider(message) => write!(f, "provider error: {message}"),
            Self::JsonRpc(message) => write!(f, "json-rpc error: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Json(error) => write!(f, "json error: {error}"),
            Self::Join(error) => write!(f, "task join error: {error}"),
        }
    }
}

impl std::error::Error for AgentCoreError {}

impl From<std::io::Error> for AgentCoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AgentCoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<reqwest::Error> for AgentCoreError {
    fn from(error: reqwest::Error) -> Self {
        Self::Provider(error.to_string())
    }
}

impl From<tokio::task::JoinError> for AgentCoreError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Join(error)
    }
}
