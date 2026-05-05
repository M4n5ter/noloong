use thiserror::Error;

pub type Result<T> = std::result::Result<T, AgentCoreError>;

#[derive(Debug, Error)]
pub enum AgentCoreError {
    #[error("run aborted")]
    Aborted,
    #[error("event sink failed: {0}")]
    EventSink(String),
    #[error("invalid effect: {0}")]
    InvalidEffect(String),
    #[error("model provider not found: {0}")]
    MissingModelProvider(String),
    #[error("tool not found: {0}")]
    MissingTool(String),
    #[error("phase failed: {0}")]
    Phase(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("{provider} request failed with status {status}: {body}")]
    HttpStatus {
        provider: String,
        status: u16,
        body: String,
    },
    #[error("json-rpc error: {0}")]
    JsonRpc(String),
    #[error("store error: {0}")]
    Store(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl From<reqwest::Error> for AgentCoreError {
    fn from(error: reqwest::Error) -> Self {
        Self::Provider(error.to_string())
    }
}
