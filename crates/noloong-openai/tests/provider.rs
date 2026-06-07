#[path = "support/jwt.rs"]
mod jwt_support;

use jwt_support::unsigned_jwt;
use noloong_openai::auth::{
    ChatGptAuthManager, ChatGptEphemeralTokenStorage, ChatGptTokenData, ChatGptTokenStore,
};
use noloong_openai::provider::{
    CHATGPT_CODEX_FALLBACK_INSTRUCTIONS, CHATGPT_CODEX_REQUEST_TIMEOUT_SECS,
    CHATGPT_CODEX_RESPONSES_BASE_URL, chatgpt_responses_provider,
    chatgpt_responses_provider_config,
};
use std::sync::Arc;

#[test]
fn provider_config_builds_chatgpt_responses_config_without_model_lock_in()
-> noloong_openai::Result<()> {
    let auth = Arc::new(auth_manager()?);

    let config = chatgpt_responses_provider_config("provider-id", "model-under-test", auth);

    assert_eq!(config.id, "provider-id");
    assert_eq!(config.model, "model-under-test");
    assert_eq!(config.base_url, CHATGPT_CODEX_RESPONSES_BASE_URL);
    assert_eq!(
        config.fallback_instructions.as_deref(),
        Some(CHATGPT_CODEX_FALLBACK_INSTRUCTIONS)
    );
    assert_eq!(config.api_key, None);
    assert_eq!(config.api_key_env, None);
    assert_eq!(
        config.auth_provider.as_ref().map(|provider| provider.id()),
        Some("openai.chatgpt")
    );
    assert_eq!(
        config.request_timeout,
        std::time::Duration::from_secs(CHATGPT_CODEX_REQUEST_TIMEOUT_SECS)
    );
    Ok(())
}

#[test]
fn provider_helper_constructs_responses_provider() -> noloong_openai::Result<()> {
    let auth = Arc::new(auth_manager()?);

    let provider = chatgpt_responses_provider("provider-id", "model-under-test", auth)
        .map_err(|error| noloong_openai::OpenAiIntegrationError::Login(error.to_string()))?;

    assert_eq!(provider.config().base_url, CHATGPT_CODEX_RESPONSES_BASE_URL);
    assert_eq!(provider.config().model, "model-under-test");
    Ok(())
}

fn auth_manager() -> noloong_openai::Result<ChatGptAuthManager> {
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&ChatGptTokenData::new(
        unsigned_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-123"
            }
        })),
        "access-token",
        "refresh-token",
        123,
    ))?;
    Ok(ChatGptAuthManager::new(storage))
}
