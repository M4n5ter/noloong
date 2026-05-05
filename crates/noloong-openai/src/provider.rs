//! Helpers for constructing OpenAI and ChatGPT-backed model providers.

use noloong_agent_core::{
    HttpAuthProvider, ResponsesApiProvider, ResponsesApiProviderConfig, Result as CoreResult,
};
use std::sync::Arc;

pub const CHATGPT_CODEX_RESPONSES_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const CHATGPT_CODEX_FALLBACK_INSTRUCTIONS: &str = "You are a helpful assistant.";

pub fn chatgpt_responses_provider_config(
    id: impl Into<String>,
    model: impl Into<String>,
    auth_provider: Arc<dyn HttpAuthProvider>,
) -> ResponsesApiProviderConfig {
    ResponsesApiProviderConfig::new(id, model)
        .base_url(CHATGPT_CODEX_RESPONSES_BASE_URL)
        .without_api_key()
        .auth_provider(auth_provider)
        .fallback_instructions(CHATGPT_CODEX_FALLBACK_INSTRUCTIONS)
}

pub fn chatgpt_responses_provider(
    id: impl Into<String>,
    model: impl Into<String>,
    auth_provider: Arc<dyn HttpAuthProvider>,
) -> CoreResult<ResponsesApiProvider> {
    ResponsesApiProvider::new(chatgpt_responses_provider_config(id, model, auth_provider))
}
