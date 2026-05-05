use super::{ChatGptLoginConfig, DeviceAuthorization, PkceCodes};
use crate::auth::{ChatGptTokenData, ChatGptTokenStore};
use crate::util::body_preview;
use crate::{OpenAiIntegrationError, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ExchangedTokens {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
}

pub async fn exchange_authorization_code(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
    redirect_uri: &str,
    pkce: &PkceCodes,
    code: &str,
) -> Result<ExchangedTokens> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("client_id", &config.client_id)
        .append_pair("code_verifier", &pkce.code_verifier)
        .finish();
    let response = client
        .post(config.token_endpoint())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    parse_token_response("oauth token endpoint", response).await
}

pub(crate) async fn exchange_device_authorization_code(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
    authorization: &DeviceAuthorization,
) -> Result<ExchangedTokens> {
    let redirect_uri = format!("{}/deviceauth/callback", config.issuer_base());
    exchange_authorization_code(
        client,
        config,
        &redirect_uri,
        &authorization.pkce,
        &authorization.authorization_code,
    )
    .await
}

pub fn token_data_from_exchange(tokens: ExchangedTokens) -> Result<ChatGptTokenData> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| OpenAiIntegrationError::Login(format!("system clock error: {error}")))?
        .as_secs();
    let mut token = ChatGptTokenData::new(
        tokens.id_token,
        tokens.access_token,
        tokens.refresh_token,
        now,
    );
    if let Some(account_id) = token.id_token_claims()?.account_id {
        token = token.account_id(account_id);
    }
    Ok(token)
}

pub fn persist_exchanged_tokens(
    store: &dyn ChatGptTokenStore,
    tokens: ExchangedTokens,
) -> Result<ChatGptTokenData> {
    let token = token_data_from_exchange(tokens)?;
    store.save(&token)?;
    Ok(token)
}

pub(crate) async fn parse_token_response(
    endpoint: &'static str,
    response: reqwest::Response,
) -> Result<ExchangedTokens> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(status_error(endpoint, status, body));
    }
    Ok(serde_json::from_str(&body)?)
}

pub(crate) fn status_error(
    endpoint: &'static str,
    status: StatusCode,
    body: String,
) -> OpenAiIntegrationError {
    OpenAiIntegrationError::EndpointStatus {
        endpoint,
        status: status.as_u16(),
        body: body_preview(&body),
    }
}
