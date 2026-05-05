use super::{ChatGptTokenClaims, ChatGptTokenData, ChatGptTokenStore};
use crate::util::body_preview;
use crate::{OpenAiIntegrationError, Result};
use noloong_agent_core::{
    AgentCoreError, BoxFuture, CancellationToken, HttpAuthContext, HttpAuthHeader, HttpAuthHeaders,
    HttpAuthProvider, HttpAuthRefreshContext, HttpAuthRefreshResult,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fmt::{Debug, Formatter},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const TOKEN_REFRESH_INTERVAL: Duration = Duration::from_secs(8 * 24 * 60 * 60);
const DEFAULT_REFRESH_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const DEFAULT_REVOKE_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/revoke";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatGptAuthManagerConfig {
    pub provider_id: String,
    pub client_id: String,
    pub refresh_endpoint: String,
    pub revoke_endpoint: String,
    pub proactive_refresh_after: Duration,
}

impl ChatGptAuthManagerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }

    pub fn client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn refresh_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.refresh_endpoint = endpoint.into();
        self
    }

    pub fn revoke_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.revoke_endpoint = endpoint.into();
        self
    }

    pub fn proactive_refresh_after(mut self, duration: Duration) -> Self {
        self.proactive_refresh_after = duration;
        self
    }
}

impl Default for ChatGptAuthManagerConfig {
    fn default() -> Self {
        Self {
            provider_id: "openai.chatgpt".into(),
            client_id: super::login::DEFAULT_CLIENT_ID.into(),
            refresh_endpoint: DEFAULT_REFRESH_TOKEN_ENDPOINT.into(),
            revoke_endpoint: DEFAULT_REVOKE_TOKEN_ENDPOINT.into(),
            proactive_refresh_after: TOKEN_REFRESH_INTERVAL,
        }
    }
}

#[derive(Clone)]
pub struct ChatGptAuthManager {
    config: ChatGptAuthManagerConfig,
    client: reqwest::Client,
    storage: Arc<dyn ChatGptTokenStore>,
    refresh_lock: Arc<Mutex<()>>,
}

impl ChatGptAuthManager {
    pub fn new(storage: Arc<dyn ChatGptTokenStore>) -> Self {
        Self::with_config(ChatGptAuthManagerConfig::default(), storage)
    }

    pub fn with_config(
        config: ChatGptAuthManagerConfig,
        storage: Arc<dyn ChatGptTokenStore>,
    ) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            storage,
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    pub fn config(&self) -> &ChatGptAuthManagerConfig {
        &self.config
    }

    pub fn storage(&self) -> &Arc<dyn ChatGptTokenStore> {
        &self.storage
    }

    pub async fn current_token(&self) -> Result<ChatGptTokenData> {
        self.storage
            .load()?
            .ok_or(OpenAiIntegrationError::MissingToken)
    }

    pub async fn auth_headers(&self) -> Result<HttpAuthHeaders> {
        let token = self.token_for_request().await?;
        Ok(HttpAuthHeaders::new(Self::headers_for_token(&token)?))
    }

    pub async fn refresh_token(&self) -> Result<ChatGptTokenData> {
        let _guard = self.refresh_lock.lock().await;
        let current = self.current_token().await?;
        self.refresh_loaded_token(current).await
    }

    async fn refresh_token_if_required(&self) -> Result<ChatGptTokenData> {
        let _guard = self.refresh_lock.lock().await;
        let current = self.current_token().await?;
        if !token_requires_proactive_refresh(&current, self.config.proactive_refresh_after)? {
            return Ok(current);
        }
        self.refresh_loaded_token(current).await
    }

    async fn refresh_loaded_token(&self, current: ChatGptTokenData) -> Result<ChatGptTokenData> {
        let response = self
            .client
            .post(&self.config.refresh_endpoint)
            .header("Content-Type", "application/json")
            .json(&RefreshRequest {
                client_id: self.config.client_id.clone(),
                grant_type: "refresh_token",
                refresh_token: current.refresh_token.clone(),
            })
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(refresh_error(status, &body));
        }
        let response = serde_json::from_str::<RefreshResponse>(&body)?;
        let refreshed = refresh_response_to_token(current, response)?;
        self.storage.save(&refreshed)?;
        Ok(refreshed)
    }

    pub async fn revoke_token(&self) -> Result<()> {
        let token = self.current_token().await?;
        let (token_value, kind) = if token.refresh_token.is_empty() {
            (token.access_token.as_str(), "access_token")
        } else {
            (token.refresh_token.as_str(), "refresh_token")
        };
        let mut request = RevokeRequest {
            token: token_value,
            token_type_hint: kind,
            client_id: None,
        };
        if kind == "refresh_token" {
            request.client_id = Some(&self.config.client_id);
        }
        let response = self
            .client
            .post(&self.config.revoke_endpoint)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(OpenAiIntegrationError::EndpointStatus {
                endpoint: "oauth revoke endpoint",
                status: status.as_u16(),
                body: body_preview(&body),
            });
        }
        self.storage.delete()?;
        Ok(())
    }

    fn headers_for_token(token: &ChatGptTokenData) -> Result<Vec<HttpAuthHeader>> {
        let claims = token.id_token_claims()?;
        let account_id = token.account_id.as_ref().or(claims.account_id.as_ref());
        let mut headers = vec![HttpAuthHeader::new(
            "Authorization",
            format!("Bearer {}", token.access_token),
        )];
        if let Some(account_id) = account_id {
            headers.push(HttpAuthHeader::new("ChatGPT-Account-ID", account_id));
        }
        if claims.fedramp {
            headers.push(HttpAuthHeader::new("X-OpenAI-Fedramp", "true"));
        }
        Ok(headers)
    }

    async fn token_for_request(&self) -> Result<ChatGptTokenData> {
        let token = self.current_token().await?;
        if token_requires_proactive_refresh(&token, self.config.proactive_refresh_after)? {
            return self.refresh_token_if_required().await;
        }
        Ok(token)
    }
}

impl Debug for ChatGptAuthManager {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptAuthManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl HttpAuthProvider for ChatGptAuthManager {
    fn id(&self) -> &str {
        &self.config.provider_id
    }

    fn headers<'a>(
        &'a self,
        _context: HttpAuthContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthHeaders> {
        Box::pin(async move { self.auth_headers().await.map_err(to_core_error) })
    }

    fn refresh<'a>(
        &'a self,
        _context: HttpAuthRefreshContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthRefreshResult> {
        Box::pin(async move {
            let token = self.refresh_token().await.map_err(to_core_error)?;
            let headers = Self::headers_for_token(&token).map_err(to_core_error)?;
            Ok(HttpAuthRefreshResult::retry_with_headers(headers))
        })
    }
}

#[derive(Serialize)]
struct RefreshRequest {
    client_id: String,
    grant_type: &'static str,
    refresh_token: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    token: &'a str,
    token_type_hint: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<&'a str>,
}

fn refresh_response_to_token(
    current: ChatGptTokenData,
    response: RefreshResponse,
) -> Result<ChatGptTokenData> {
    let now = unix_timestamp()?;
    let id_token = response.id_token.unwrap_or(current.id_token);
    let access_token = response.access_token.unwrap_or(current.access_token);
    let refresh_token = response.refresh_token.unwrap_or(current.refresh_token);
    let mut token = ChatGptTokenData::new(id_token, access_token, refresh_token, now);
    let claims = token.id_token_claims()?;
    if let Some(account_id) = claims.account_id.or(current.account_id) {
        token = token.account_id(account_id);
    }
    Ok(token)
}

fn token_requires_proactive_refresh(token: &ChatGptTokenData, interval: Duration) -> Result<bool> {
    let now = unix_timestamp()?;
    if let Ok(claims) = ChatGptTokenClaims::from_jwt(&token.access_token)
        && let Some(exp) = claims.exp
        && exp <= now
    {
        return Ok(true);
    }
    Ok(token.last_refresh.saturating_add(interval.as_secs()) <= now)
}

fn refresh_error(status: StatusCode, body: &str) -> OpenAiIntegrationError {
    if status == StatusCode::UNAUTHORIZED {
        return OpenAiIntegrationError::RefreshTokenPermanent(
            classify_refresh_failure(body).to_string(),
        );
    }
    OpenAiIntegrationError::EndpointStatus {
        endpoint: "oauth refresh endpoint",
        status: status.as_u16(),
        body: body_preview(body),
    }
}

fn classify_refresh_failure(body: &str) -> &'static str {
    match refresh_error_code(body)
        .as_deref()
        .map(str::to_ascii_lowercase)
    {
        Some(code) if code == "refresh_token_expired" => "refresh token expired",
        Some(code) if code == "refresh_token_reused" => "refresh token reused",
        Some(code) if code == "refresh_token_invalidated" => "refresh token revoked",
        _ => "refresh token invalid",
    }
}

fn refresh_error_code(body: &str) -> Option<String> {
    let Value::Object(map) = serde_json::from_str::<Value>(body).ok()? else {
        return None;
    };
    match map.get("error") {
        Some(Value::Object(error)) => error
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some(Value::String(error)) => Some(error.clone()),
        _ => map.get("code").and_then(Value::as_str).map(str::to_string),
    }
}

fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| OpenAiIntegrationError::Login(format!("system clock error: {error}")))?
        .as_secs())
}

fn to_core_error(error: OpenAiIntegrationError) -> AgentCoreError {
    AgentCoreError::Provider(error.to_string())
}
