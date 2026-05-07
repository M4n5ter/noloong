mod support;

use noloong_agent_core::{
    CancellationToken, HttpAuthContext, HttpAuthProvider, HttpAuthRefreshContext,
};
use noloong_openai::OpenAiIntegrationError;
use noloong_openai::auth::{
    ChatGptAuthManager, ChatGptAuthManagerConfig, ChatGptEphemeralTokenStorage, ChatGptTokenData,
    ChatGptTokenStore,
};
use serde_json::json;
use std::{sync::Arc, time::SystemTime};
use support::{MockHttpServer, MockResponse, unsigned_jwt};

#[tokio::test]
async fn auth_manager_headers_include_bearer_account_and_fedramp() -> noloong_openai::Result<()> {
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&sample_token(
        "access-token",
        "refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = ChatGptAuthManager::new(storage);

    let headers = manager.auth_headers().await?.headers;

    assert_header(&headers, "Authorization", "Bearer access-token");
    assert_header(&headers, "ChatGPT-Account-ID", "account-123");
    assert_header(&headers, "X-OpenAI-Fedramp", "true");
    Ok(())
}

#[tokio::test]
async fn auth_manager_missing_token_error_points_to_login() -> noloong_openai::Result<()> {
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    let manager = ChatGptAuthManager::new(storage);

    let error = manager
        .auth_headers()
        .await
        .expect_err("missing token should be reported");

    assert!(
        error
            .to_string()
            .contains("noloong chatgpt login --flow browser")
    );
    Ok(())
}

#[tokio::test]
async fn auth_manager_proactively_refreshes_expired_access_token() -> noloong_openai::Result<()> {
    let refreshed_id_token = id_token("account-123", true);
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({
            "id_token": refreshed_id_token,
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token"
        }),
    )])
    .await?;
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&ChatGptTokenData::new(
        id_token("account-123", true),
        unsigned_jwt(json!({ "exp": 1_u64 })),
        "old-refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = manager_for_server(&server, Arc::clone(&storage));

    let headers = manager.auth_headers().await?.headers;

    assert_header(&headers, "Authorization", "Bearer new-access-token");
    assert_eq!(
        storage
            .load()?
            .as_ref()
            .map(|token| token.refresh_token.as_str()),
        Some("new-refresh-token")
    );
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /oauth/token "));
    assert!(requests[0].contains(r#""grant_type":"refresh_token""#));
    assert!(requests[0].contains(r#""refresh_token":"old-refresh-token""#));
    Ok(())
}

#[tokio::test]
async fn auth_manager_coalesces_concurrent_proactive_refresh() -> noloong_openai::Result<()> {
    let refreshed_id_token = id_token("account-123", true);
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({
            "id_token": refreshed_id_token,
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token"
        }),
    )])
    .await?;
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&ChatGptTokenData::new(
        id_token("account-123", true),
        unsigned_jwt(json!({ "exp": 1_u64 })),
        "old-refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = Arc::new(manager_for_server(&server, Arc::clone(&storage)));

    let (left, right) = tokio::join!(manager.auth_headers(), manager.auth_headers());
    let left = left?;
    let right = right?;

    assert_header(&left.headers, "Authorization", "Bearer new-access-token");
    assert_header(&right.headers, "Authorization", "Bearer new-access-token");
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn auth_manager_http_auth_refresh_returns_retry_headers() -> noloong_openai::Result<()> {
    let refreshed_id_token = id_token("account-456", false);
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({
            "id_token": refreshed_id_token,
            "access_token": "retry-access-token",
            "refresh_token": "retry-refresh-token"
        }),
    )])
    .await?;
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&sample_token(
        "old-access-token",
        "old-refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = manager_for_server(&server, storage);

    let result = manager
        .refresh(
            HttpAuthRefreshContext::unauthorized(
                HttpAuthContext::new("openai.chatgpt", "POST", "https://example.test", 0),
                401,
            ),
            CancellationToken::new(),
        )
        .await
        .map_err(|error| OpenAiIntegrationError::Login(error.to_string()))?;

    assert!(result.retry);
    let headers = result.headers.expect("retry headers");
    assert_header(&headers, "Authorization", "Bearer retry-access-token");
    assert_header(&headers, "ChatGPT-Account-ID", "account-456");
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn auth_manager_permanent_refresh_failure_is_structured() -> noloong_openai::Result<()> {
    let server = MockHttpServer::spawn(vec![MockResponse::text(
        401,
        r#"{"error":{"code":"refresh_token_expired"}}"#,
    )])
    .await?;
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&sample_token(
        "old-access-token",
        "old-refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = manager_for_server(&server, storage);

    let error = manager
        .refresh_token()
        .await
        .expect_err("expired refresh token should fail permanently");

    assert!(matches!(
        error,
        OpenAiIntegrationError::RefreshTokenPermanent(message)
            if message == "refresh token expired"
    ));
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    Ok(())
}

#[tokio::test]
async fn auth_manager_revoke_deletes_local_token_after_success() -> noloong_openai::Result<()> {
    let server = MockHttpServer::spawn(vec![MockResponse::json(200, json!({}))]).await?;
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&sample_token(
        "access-token",
        "refresh-token",
        unix_timestamp()?,
    ))?;
    let manager = manager_for_server(&server, Arc::clone(&storage));

    manager.revoke_token().await?;

    assert_eq!(storage.load()?, None);
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /oauth/revoke "));
    assert!(requests[0].contains(r#""token":"refresh-token""#));
    assert!(requests[0].contains(r#""token_type_hint":"refresh_token""#));
    assert!(requests[0].contains(r#""client_id":"client-id""#));
    Ok(())
}

fn manager_for_server(
    server: &MockHttpServer,
    storage: Arc<ChatGptEphemeralTokenStorage>,
) -> ChatGptAuthManager {
    ChatGptAuthManager::with_config(
        ChatGptAuthManagerConfig::new()
            .client_id("client-id")
            .refresh_endpoint(format!("{}/oauth/token", server.base_url()))
            .revoke_endpoint(format!("{}/oauth/revoke", server.base_url())),
        storage,
    )
}

fn sample_token(
    access_token: impl Into<String>,
    refresh_token: impl Into<String>,
    last_refresh: u64,
) -> ChatGptTokenData {
    ChatGptTokenData::new(
        id_token("account-123", true),
        access_token,
        refresh_token,
        last_refresh,
    )
}

fn id_token(account_id: &str, fedramp: bool) -> String {
    unsigned_jwt(json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "chatgpt_account_is_fedramp": fedramp
        }
    }))
}

fn assert_header(headers: &[noloong_agent_core::HttpAuthHeader], name: &str, value: &str) {
    assert!(
        headers
            .iter()
            .any(|header| header.name == name && header.value == value),
        "missing header {name}: {value}; got {headers:?}"
    );
}

fn unix_timestamp() -> noloong_openai::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| noloong_openai::OpenAiIntegrationError::Login(error.to_string()))?
        .as_secs())
}
