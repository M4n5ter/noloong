pub use crate::polling::TelegramUpdate;
use crate::{network::TelegramNetworkConfig, polling::TelegramApiResponse};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{error::Error as StdError, future::Future, pin::Pin};
use thiserror::Error;

pub type TelegramApiFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TelegramApiError>> + Send + 'a>>;

pub trait TelegramApi: Send + Sync {
    fn get_updates<'a>(
        &'a self,
        offset: Option<i64>,
        timeout_seconds: u64,
    ) -> TelegramApiFuture<'a, Vec<TelegramUpdate>>;

    fn send_message<'a>(
        &'a self,
        request: TelegramSendMessageRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle>;

    fn edit_message_text<'a>(
        &'a self,
        request: TelegramEditMessageTextRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle>;

    fn answer_callback_query<'a>(
        &'a self,
        callback_query_id: &'a str,
        text: Option<&'a str>,
    ) -> TelegramApiFuture<'a, ()>;
}

#[derive(Clone)]
pub struct ReqwestTelegramApi {
    client: Client,
    base_url: String,
    token: String,
}

impl ReqwestTelegramApi {
    pub fn new(client: Client, token: impl Into<String>, network: &TelegramNetworkConfig) -> Self {
        Self {
            client,
            base_url: network
                .api_base_url
                .clone()
                .unwrap_or_else(|| "https://api.telegram.org".into()),
            token: token.into(),
        }
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.base_url.trim_end_matches('/'),
            self.token,
            method
        )
    }

    fn network_error(&self, method: &str, error: reqwest::Error) -> TelegramApiError {
        let mut details = vec![error.to_string()];
        let mut source = error.source();
        while let Some(error) = source {
            details.push(error.to_string());
            source = error.source();
        }
        let reason = details.join(": ").replace(&self.token, "<redacted>");
        TelegramApiError::Network(format!("{method} request failed: {reason}"))
    }
}

impl TelegramApi for ReqwestTelegramApi {
    fn get_updates<'a>(
        &'a self,
        offset: Option<i64>,
        timeout_seconds: u64,
    ) -> TelegramApiFuture<'a, Vec<TelegramUpdate>> {
        Box::pin(async move {
            let mut body = serde_json::json!({
                "timeout": timeout_seconds,
                "allowed_updates": ["message", "callback_query"],
            });
            if let Some(offset) = offset {
                body["offset"] = serde_json::json!(offset);
            }
            let response = self
                .client
                .post(self.method_url("getUpdates"))
                .json(&body)
                .send()
                .await
                .map_err(|error| self.network_error("getUpdates", error))?;
            parse_telegram_response(response).await
        })
    }

    fn send_message<'a>(
        &'a self,
        request: TelegramSendMessageRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            let response = self
                .client
                .post(self.method_url("sendMessage"))
                .json(&request)
                .send()
                .await
                .map_err(|error| self.network_error("sendMessage", error))?;
            let message = parse_telegram_response::<TelegramSentMessage>(response).await?;
            Ok(TelegramMessageHandle {
                chat_id: message.chat.id,
                message_id: message.message_id,
            })
        })
    }

    fn edit_message_text<'a>(
        &'a self,
        request: TelegramEditMessageTextRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            let response = self
                .client
                .post(self.method_url("editMessageText"))
                .json(&request)
                .send()
                .await
                .map_err(|error| self.network_error("editMessageText", error))?;
            let message = parse_telegram_response::<TelegramSentMessage>(response).await?;
            Ok(TelegramMessageHandle {
                chat_id: message.chat.id,
                message_id: message.message_id,
            })
        })
    }

    fn answer_callback_query<'a>(
        &'a self,
        callback_query_id: &'a str,
        text: Option<&'a str>,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self
                .client
                .post(self.method_url("answerCallbackQuery"))
                .json(&serde_json::json!({
                    "callback_query_id": callback_query_id,
                    "text": text,
                }))
                .send()
                .await
                .map_err(|error| self.network_error("answerCallbackQuery", error))?;
            parse_telegram_response::<bool>(response).await.map(|_| ())
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramSendMessageRequest {
    pub chat_id: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<TelegramParseMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramEditMessageTextRequest {
    pub chat_id: i64,
    pub message_id: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<TelegramParseMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TelegramParseMode {
    #[serde(rename = "MarkdownV2")]
    MarkdownV2,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramInlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<TelegramInlineKeyboardButton>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramInlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramMessageHandle {
    pub chat_id: i64,
    pub message_id: i64,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TelegramApiError {
    #[error("telegram network error: {0}")]
    Network(String),
    #[error("telegram api error {code}: {description}")]
    Api { code: i64, description: String },
    #[error("telegram response decode failed: {0}")]
    Decode(String),
}

impl TelegramApiError {
    pub fn is_parse_error(&self) -> bool {
        matches!(self, Self::Api { description, .. } if description.contains("parse entities") || description.contains("can't parse"))
    }

    pub fn is_message_not_modified(&self) -> bool {
        matches!(self, Self::Api { description, .. } if description.contains("message is not modified"))
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Api { code: 409, .. })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
struct TelegramSentMessage {
    message_id: i64,
    chat: TelegramSentChat,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct TelegramSentChat {
    id: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct TelegramErrorResponse {
    error_code: i64,
    description: String,
}

async fn parse_telegram_response<T>(response: reqwest::Response) -> Result<T, TelegramApiError>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| TelegramApiError::Network(error.to_string()))?;
    if !status.is_success() {
        let error = serde_json::from_slice::<TelegramErrorResponse>(&bytes)
            .map_err(|error| TelegramApiError::Decode(error.to_string()))?;
        return Err(TelegramApiError::Api {
            code: error.error_code,
            description: error.description,
        });
    }
    let body = serde_json::from_slice::<TelegramApiResponse<T>>(&bytes)
        .map_err(|error| TelegramApiError::Decode(error.to_string()))?;
    Ok(body.result)
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramApiResponse, TelegramEditMessageTextRequest, TelegramInlineKeyboardButton,
        TelegramInlineKeyboardMarkup, TelegramParseMode, TelegramSendMessageRequest,
        TelegramSentMessage,
    };
    use serde_json::json;

    #[test]
    fn send_message_request_serializes_bot_api_snake_case() {
        let request = TelegramSendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: Some(TelegramParseMode::MarkdownV2),
            reply_markup: Some(TelegramInlineKeyboardMarkup {
                inline_keyboard: vec![vec![TelegramInlineKeyboardButton {
                    text: "Allow".into(),
                    callback_data: "approve:1".into(),
                }]],
            }),
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value,
            json!({
                "chat_id": 42,
                "text": "hello",
                "parse_mode": "MarkdownV2",
                "reply_markup": {
                    "inline_keyboard": [[{
                        "text": "Allow",
                        "callback_data": "approve:1"
                    }]]
                }
            })
        );
    }

    #[test]
    fn edit_message_request_serializes_required_message_id() {
        let request = TelegramEditMessageTextRequest {
            chat_id: 42,
            message_id: 7,
            text: "hello".into(),
            parse_mode: Some(TelegramParseMode::MarkdownV2),
            reply_markup: None,
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value,
            json!({
                "chat_id": 42,
                "message_id": 7,
                "text": "hello",
                "parse_mode": "MarkdownV2"
            })
        );
    }

    #[test]
    fn sent_message_response_deserializes_bot_api_snake_case() {
        let response = serde_json::from_value::<TelegramApiResponse<TelegramSentMessage>>(json!({
            "ok": true,
            "result": {
                "message_id": 9,
                "chat": {"id": 42}
            }
        }))
        .unwrap();

        assert_eq!(response.result.message_id, 9);
        assert_eq!(response.result.chat.id, 42);
    }
}
