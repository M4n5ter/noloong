mod support;

use noloong_openai::OpenAiIntegrationError;
use noloong_openai::auth::{
    BrowserCallback, BrowserLoginServer, BrowserLoginSession, ChatGptDeviceCode,
    ChatGptEphemeralTokenStorage, ChatGptLoginConfig, ChatGptTokenStore, DeviceAuthorizationStatus,
    PkceCodes, code_challenge_for_verifier, complete_device_authorization,
    exchange_authorization_code, persist_exchanged_tokens, poll_device_authorization,
    request_device_authorization,
};
use serde_json::json;
use support::{MockHttpServer, MockResponse, unsigned_jwt};

#[test]
fn login_browser_session_builds_codex_compatible_authorize_url() -> noloong_openai::Result<()> {
    let config = ChatGptLoginConfig::new()
        .issuer("https://auth.openai.com/")
        .client_id("client-id")
        .forced_workspace_id("workspace-123");
    let pkce = PkceCodes::from_verifier("fixed-verifier");
    let session = BrowserLoginSession::with_pkce_state(&config, 1455, pkce, "state-123")?;
    let url = url::Url::parse(&session.authorization_url)?;
    let query = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(
        url.as_str().split('?').next(),
        Some("https://auth.openai.com/oauth/authorize")
    );
    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        query.get("client_id").map(String::as_str),
        Some("client-id")
    );
    assert_eq!(
        query.get("scope").map(String::as_str),
        Some("openid profile email offline_access api.connectors.read api.connectors.invoke")
    );
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert_eq!(
        query.get("code_challenge").map(String::as_str),
        Some(code_challenge_for_verifier("fixed-verifier").as_str())
    );
    assert_eq!(
        query.get("allowed_workspace_id").map(String::as_str),
        Some("workspace-123")
    );
    assert_eq!(
        query.get("originator").map(String::as_str),
        Some("codex_cli_rs")
    );
    assert_eq!(session.redirect_uri, "http://localhost:1455/auth/callback");
    Ok(())
}

#[tokio::test]
async fn login_browser_server_falls_back_when_preferred_port_is_busy() -> noloong_openai::Result<()>
{
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let preferred_port = occupied.local_addr()?.port();
    let config = ChatGptLoginConfig::new()
        .preferred_callback_port(preferred_port)
        .fallback_callback_port(0);

    let server = BrowserLoginServer::bind(config).await?;

    assert_ne!(server.session().port, preferred_port);
    assert_ne!(server.session().port, 0);
    Ok(())
}

#[test]
fn login_browser_callback_rejects_state_mismatch() {
    let error = BrowserCallback::from_redirect_url(
        "http://localhost:1455/auth/callback?code=code-123&state=wrong",
        "expected",
    )
    .expect_err("mismatched callback state must be rejected");

    assert!(matches!(error, OpenAiIntegrationError::OAuthStateMismatch));
}

#[tokio::test]
async fn login_exchanges_authorization_code_and_persists_token() -> noloong_openai::Result<()> {
    let id_token = unsigned_jwt(json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "account-123"
        }
    }));
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({
            "id_token": id_token,
            "access_token": "access-token",
            "refresh_token": "refresh-token"
        }),
    )])
    .await?;
    let config = ChatGptLoginConfig::new()
        .issuer(server.base_url())
        .client_id("client-id");
    let client = reqwest::Client::new();
    let pkce = PkceCodes::from_verifier("verifier");

    let tokens = exchange_authorization_code(
        &client,
        &config,
        "http://localhost:1455/auth/callback",
        &pkce,
        "authorization-code",
    )
    .await?;
    let storage = ChatGptEphemeralTokenStorage::new();
    let token = persist_exchanged_tokens(&storage, tokens)?;

    assert_eq!(token.account_id.as_deref(), Some("account-123"));
    assert_eq!(
        storage
            .load()?
            .as_ref()
            .and_then(|data| data.account_id.as_deref()),
        Some("account-123")
    );
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /oauth/token "));
    assert!(requests[0].contains("grant_type=authorization_code"));
    assert!(requests[0].contains("code=authorization-code"));
    assert!(requests[0].contains("client_id=client-id"));
    assert!(requests[0].contains("code_verifier=verifier"));
    Ok(())
}

#[tokio::test]
async fn login_device_code_request_and_poll_pending() -> noloong_openai::Result<()> {
    let server = MockHttpServer::spawn(vec![
        MockResponse::json(
            200,
            json!({
                "device_auth_id": "device-123",
                "user_code": "USER-CODE",
                "interval": "0"
            }),
        ),
        MockResponse::text(403, ""),
    ])
    .await?;
    let config = ChatGptLoginConfig::new()
        .issuer(server.base_url())
        .client_id("client-id");
    let client = reqwest::Client::new();

    let device_code = request_device_authorization(&client, &config).await?;
    let poll = poll_device_authorization(&client, &config, &device_code).await?;

    assert_eq!(
        device_code.verification_url,
        format!("{}/codex/device", server.base_url())
    );
    assert_eq!(device_code.user_code, "USER-CODE");
    assert!(matches!(poll.status, DeviceAuthorizationStatus::Pending));
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /api/accounts/deviceauth/usercode "));
    assert!(requests[1].starts_with("POST /api/accounts/deviceauth/token "));
    Ok(())
}

#[tokio::test]
async fn login_device_code_completion_exchanges_and_saves_token() -> noloong_openai::Result<()> {
    let id_token = unsigned_jwt(json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "account-device"
        }
    }));
    let server = MockHttpServer::spawn(vec![
        MockResponse::json(
            200,
            json!({
                "authorization_code": "authorization-code",
                "code_challenge": code_challenge_for_verifier("device-verifier"),
                "code_verifier": "device-verifier"
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "id_token": id_token,
                "access_token": "access-token",
                "refresh_token": "refresh-token"
            }),
        ),
    ])
    .await?;
    let config = ChatGptLoginConfig::new()
        .issuer(server.base_url())
        .client_id("client-id");
    let client = reqwest::Client::new();
    let storage = ChatGptEphemeralTokenStorage::new();

    let token = complete_device_authorization(
        &client,
        &config,
        ChatGptDeviceCode {
            verification_url: format!("{}/codex/device", server.base_url()),
            user_code: "USER-CODE".into(),
            device_auth_id: "device-123".into(),
            interval_secs: 0,
        },
        &storage,
    )
    .await?;

    assert_eq!(token.account_id.as_deref(), Some("account-device"));
    assert_eq!(
        storage
            .load()?
            .as_ref()
            .and_then(|data| data.account_id.as_deref()),
        Some("account-device")
    );
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /api/accounts/deviceauth/token "));
    assert!(requests[1].starts_with("POST /oauth/token "));
    assert!(requests[1].contains("redirect_uri="));
    Ok(())
}

#[tokio::test]
async fn login_device_code_timeout_reports_timeout() -> noloong_openai::Result<()> {
    let server = MockHttpServer::spawn(vec![MockResponse::text(403, "")]).await?;
    let config = ChatGptLoginConfig::new()
        .issuer(server.base_url())
        .device_poll_timeout(std::time::Duration::ZERO);
    let client = reqwest::Client::new();
    let storage = ChatGptEphemeralTokenStorage::new();

    let error = complete_device_authorization(
        &client,
        &config,
        ChatGptDeviceCode {
            verification_url: format!("{}/codex/device", server.base_url()),
            user_code: "USER-CODE".into(),
            device_auth_id: "device-123".into(),
            interval_secs: 0,
        },
        &storage,
    )
    .await
    .expect_err("pending device authorization must time out");

    assert!(matches!(
        error,
        OpenAiIntegrationError::DeviceAuthorizationTimeout
    ));
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    Ok(())
}
