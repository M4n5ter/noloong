//! OpenAI and ChatGPT integration crate for Noloong.
//!
//! This crate intentionally sits outside `noloong-agent-core`. Core owns the
//! provider-neutral traits and wire contracts; this crate will own OpenAI and
//! ChatGPT-specific auth, provider helpers, and compact endpoint integration.

pub mod auth;
pub mod compact;
pub mod provider;
mod util;

pub type Result<T> = std::result::Result<T, OpenAiIntegrationError>;

#[derive(Debug, thiserror::Error)]
pub enum OpenAiIntegrationError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("randomness error: {0}")]
    Random(#[from] getrandom::Error),
    #[error("invalid JWT: {0}")]
    InvalidJwt(String),
    #[error("OAuth state mismatch")]
    OAuthStateMismatch,
    #[error("OAuth callback did not include an authorization code")]
    MissingAuthorizationCode,
    #[error("ChatGPT token storage did not contain tokens")]
    MissingToken,
    #[error("permanent refresh token failure: {0}")]
    RefreshTokenPermanent(String),
    #[error("{endpoint} returned status {status}: {body}")]
    EndpointStatus {
        endpoint: &'static str,
        status: u16,
        body: String,
    },
    #[error("device authorization timed out")]
    DeviceAuthorizationTimeout,
    #[error("login error: {0}")]
    Login(String),
    #[error("token storage error: {0}")]
    Storage(String),
}
