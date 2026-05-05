use crate::{OpenAiIntegrationError, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Debug, Formatter};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptTokenData {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: Option<String>,
    pub last_refresh: u64,
}

impl ChatGptTokenData {
    pub fn new(
        id_token: impl Into<String>,
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        last_refresh: u64,
    ) -> Self {
        Self {
            id_token: id_token.into(),
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            account_id: None,
            last_refresh,
        }
    }

    pub fn account_id(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = Some(account_id.into());
        self
    }

    pub fn id_token_claims(&self) -> Result<ChatGptTokenClaims> {
        ChatGptTokenClaims::from_jwt(&self.id_token)
    }
}

impl Debug for ChatGptTokenData {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptTokenData")
            .field("id_token", &"<redacted>")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("account_id", &self.account_id)
            .field("last_refresh", &self.last_refresh)
            .finish()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptTokenClaims {
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub account_id: Option<String>,
    pub fedramp: bool,
    pub exp: Option<u64>,
    #[serde(default)]
    pub raw: serde_json::Map<String, Value>,
}

impl ChatGptTokenClaims {
    pub fn from_jwt(jwt: &str) -> Result<Self> {
        let payload = jwt
            .split('.')
            .nth(1)
            .ok_or_else(|| OpenAiIntegrationError::InvalidJwt("JWT payload is missing".into()))?;
        let decoded = URL_SAFE_NO_PAD.decode(payload).map_err(|error| {
            OpenAiIntegrationError::InvalidJwt(format!("JWT payload is not base64url: {error}"))
        })?;
        let raw = serde_json::from_slice::<serde_json::Map<String, Value>>(&decoded)?;
        let auth = raw
            .get("https://api.openai.com/auth")
            .and_then(Value::as_object);
        let profile = raw
            .get("https://api.openai.com/profile")
            .and_then(Value::as_object);
        Ok(Self {
            email: string_claim(&raw, &["email"])
                .or_else(|| profile.and_then(|claims| string_claim(claims, &["email"]))),
            plan_type: string_claim(
                &raw,
                &[
                    "https://api.openai.com/auth/plan_type",
                    "plan_type",
                    "planType",
                ],
            )
            .or_else(|| {
                auth.and_then(|claims| {
                    string_claim(claims, &["chatgpt_plan_type", "plan_type", "planType"])
                })
            }),
            chatgpt_user_id: string_claim(
                &raw,
                &[
                    "https://api.openai.com/auth/user_id",
                    "chatgpt_user_id",
                    "user_id",
                    "sub",
                ],
            )
            .or_else(|| {
                auth.and_then(|claims| string_claim(claims, &["chatgpt_user_id", "user_id", "sub"]))
            }),
            account_id: string_claim(
                &raw,
                &[
                    "https://api.openai.com/auth/account_id",
                    "account_id",
                    "accountId",
                ],
            )
            .or_else(|| {
                auth.and_then(|claims| {
                    string_claim(claims, &["chatgpt_account_id", "account_id", "accountId"])
                })
            }),
            fedramp: bool_claim(
                &raw,
                &[
                    "https://api.openai.com/auth/fedramp",
                    "fedramp",
                    "is_fedramp",
                    "isFedramp",
                ],
            ) || auth
                .map(|claims| {
                    bool_claim(
                        claims,
                        &[
                            "chatgpt_account_is_fedramp",
                            "chatgpt_account_fedramp",
                            "fedramp",
                        ],
                    )
                })
                .unwrap_or(false),
            exp: u64_claim(&raw, &["exp"]),
            raw,
        })
    }
}

fn string_claim(raw: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str).map(str::to_string))
}

fn bool_claim(raw: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_bool))
        .unwrap_or(false)
}

fn u64_claim(raw: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_u64))
}
