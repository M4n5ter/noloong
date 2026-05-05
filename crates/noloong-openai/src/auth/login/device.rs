use super::exchange::{exchange_device_authorization_code, persist_exchanged_tokens, status_error};
use super::{ChatGptLoginConfig, PkceCodes};
use crate::auth::{ChatGptTokenData, ChatGptTokenStore};
use crate::{OpenAiIntegrationError, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize, de};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatGptDeviceCode {
    pub verification_url: String,
    pub user_code: String,
    pub device_auth_id: String,
    pub interval_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAuthorization {
    pub authorization_code: String,
    pub pkce: PkceCodes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceAuthorizationStatus {
    Pending,
    Authorized(DeviceAuthorization),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAuthorizationPoll {
    pub status: DeviceAuthorizationStatus,
}

pub async fn request_device_authorization(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
) -> Result<ChatGptDeviceCode> {
    let response = client
        .post(format!("{}/deviceauth/usercode", config.device_api_base()))
        .json(&UserCodeRequest {
            client_id: config.client_id.clone(),
        })
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(status_error("device user-code endpoint", status, body));
    }
    let response = serde_json::from_str::<UserCodeResponse>(&body)?;
    Ok(ChatGptDeviceCode {
        verification_url: format!("{}/codex/device", config.issuer_base()),
        user_code: response.user_code,
        device_auth_id: response.device_auth_id,
        interval_secs: response.interval_secs,
    })
}

pub async fn poll_device_authorization(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
    device_code: &ChatGptDeviceCode,
) -> Result<DeviceAuthorizationPoll> {
    let response = client
        .post(format!("{}/deviceauth/token", config.device_api_base()))
        .json(&TokenPollRequest {
            device_auth_id: device_code.device_auth_id.clone(),
            user_code: device_code.user_code.clone(),
        })
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        return Ok(DeviceAuthorizationPoll {
            status: DeviceAuthorizationStatus::Pending,
        });
    }
    if !status.is_success() {
        return Err(status_error("device token endpoint", status, body));
    }
    let response = serde_json::from_str::<TokenPollResponse>(&body)?;
    Ok(DeviceAuthorizationPoll {
        status: DeviceAuthorizationStatus::Authorized(DeviceAuthorization {
            authorization_code: response.authorization_code,
            pkce: PkceCodes {
                code_verifier: response.code_verifier,
                code_challenge: response.code_challenge,
            },
        }),
    })
}

pub async fn complete_device_authorization(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
    device_code: ChatGptDeviceCode,
    store: &dyn ChatGptTokenStore,
) -> Result<ChatGptTokenData> {
    let started = Instant::now();
    loop {
        match poll_device_authorization(client, config, &device_code)
            .await?
            .status
        {
            DeviceAuthorizationStatus::Authorized(authorization) => {
                let tokens =
                    exchange_device_authorization_code(client, config, &authorization).await?;
                return persist_exchanged_tokens(store, tokens);
            }
            DeviceAuthorizationStatus::Pending => {
                if started.elapsed() >= config.device_poll_timeout {
                    return Err(OpenAiIntegrationError::DeviceAuthorizationTimeout);
                }
                let remaining = config.device_poll_timeout - started.elapsed();
                let delay =
                    std::time::Duration::from_secs(device_code.interval_secs).min(remaining);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct UserCodeRequest {
    client_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct UserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(
        default = "default_interval_secs",
        alias = "interval",
        deserialize_with = "deserialize_interval_secs"
    )]
    interval_secs: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct TokenPollRequest {
    device_auth_id: String,
    user_code: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct TokenPollResponse {
    authorization_code: String,
    code_challenge: String,
    code_verifier: String,
}

fn default_interval_secs() -> u64 {
    5
}

fn deserialize_interval_secs<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawInterval {
        String(String),
        Number(u64),
    }

    match Option::<RawInterval>::deserialize(deserializer)? {
        Some(RawInterval::String(value)) => value.trim().parse::<u64>().map_err(de::Error::custom),
        Some(RawInterval::Number(value)) => Ok(value),
        None => Ok(default_interval_secs()),
    }
}
