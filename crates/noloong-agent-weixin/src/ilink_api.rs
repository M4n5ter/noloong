use crate::config::{ILINK_BASE_URL, WEIXIN_CDN_BASE_URL};
use base64::{Engine as _, engine::general_purpose};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{error::Error as StdError, future::Future, pin::Pin, time::Duration};
use thiserror::Error;
use tokio::time::sleep;
use uuid::Uuid;

pub const CHANNEL_VERSION: &str = "2.2.0";
pub const ILINK_APP_ID: &str = "bot";
pub const ILINK_APP_CLIENT_VERSION: u32 = (2 << 16) | (2 << 8);
pub const EP_GET_UPDATES: &str = "ilink/bot/getupdates";
pub const EP_SEND_MESSAGE: &str = "ilink/bot/sendmessage";
pub const EP_SEND_TYPING: &str = "ilink/bot/sendtyping";
pub const EP_GET_CONFIG: &str = "ilink/bot/getconfig";
pub const EP_GET_UPLOAD_URL: &str = "ilink/bot/getuploadurl";
pub const EP_GET_BOT_QR: &str = "ilink/bot/get_bot_qrcode";
pub const EP_GET_QR_STATUS: &str = "ilink/bot/get_qrcode_status";
pub const LONG_POLL_TIMEOUT_MS: u64 = 35_000;
pub const API_TIMEOUT_MS: u64 = 15_000;
pub const CONFIG_TIMEOUT_MS: u64 = 10_000;
pub const QR_TIMEOUT_MS: u64 = 35_000;
pub const SESSION_EXPIRED_ERRCODE: i64 = -14;
pub const RATE_LIMIT_ERRCODE: i64 = -2;
pub const ITEM_TEXT: i64 = 1;
pub const ITEM_IMAGE: i64 = 2;
pub const ITEM_VOICE: i64 = 3;
pub const ITEM_FILE: i64 = 4;
pub const ITEM_VIDEO: i64 = 5;
pub const MSG_TYPE_USER: i64 = 1;
pub const MSG_TYPE_BOT: i64 = 2;
pub const MSG_STATE_FINISH: i64 = 2;
pub const TYPING_START: i64 = 1;
pub const TYPING_STOP: i64 = 2;
pub const MEDIA_ENCRYPT_TYPE_AES: i64 = 1;

pub type WeixinApiFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WeixinApiError>> + Send + 'a>>;

pub trait WeixinApi: Send + Sync {
    fn get_updates<'a>(
        &'a self,
        sync_buf: &'a str,
        timeout_ms: u64,
    ) -> WeixinApiFuture<'a, WeixinUpdatesResponse>;

    fn send_message<'a>(
        &'a self,
        request: WeixinSendMessageRequest,
    ) -> WeixinApiFuture<'a, WeixinSendMessageResponse>;

    fn send_typing<'a>(
        &'a self,
        request: WeixinSendTypingRequest,
    ) -> WeixinApiFuture<'a, WeixinRawResponse>;

    fn get_config<'a>(
        &'a self,
        request: WeixinGetConfigRequest,
    ) -> WeixinApiFuture<'a, WeixinConfigResponse>;

    fn get_upload_url<'a>(
        &'a self,
        request: WeixinGetUploadUrlRequest,
    ) -> WeixinApiFuture<'a, WeixinUploadUrlResponse>;

    fn upload_ciphertext<'a>(
        &'a self,
        upload_url: &'a str,
        ciphertext: Vec<u8>,
    ) -> WeixinApiFuture<'a, String>;

    fn download_bytes<'a>(&'a self, url: &'a str) -> WeixinApiFuture<'a, Vec<u8>>;

    fn get_bot_qrcode<'a>(&'a self, bot_type: &'a str) -> WeixinApiFuture<'a, WeixinQrCode>;

    fn get_qrcode_status<'a>(&'a self, qrcode: &'a str) -> WeixinApiFuture<'a, WeixinQrStatus>;
}

#[derive(Clone)]
pub struct ReqwestWeixinApi {
    client: reqwest::Client,
    base_url: String,
    cdn_base_url: String,
    token: Option<String>,
    max_download_bytes: Option<usize>,
}

impl ReqwestWeixinApi {
    pub fn new(client: reqwest::Client, token: impl Into<Option<String>>) -> Self {
        Self {
            client,
            base_url: ILINK_BASE_URL.into(),
            cdn_base_url: WEIXIN_CDN_BASE_URL.into(),
            token: token.into(),
            max_download_bytes: None,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_cdn_base_url(mut self, cdn_base_url: impl Into<String>) -> Self {
        self.cdn_base_url = cdn_base_url.into();
        self
    }

    pub fn with_max_download_bytes(mut self, max_download_bytes: usize) -> Self {
        self.max_download_bytes = Some(max_download_bytes);
        self
    }

    pub fn cdn_base_url(&self) -> &str {
        &self.cdn_base_url
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        )
    }

    fn cdn_url(&self, path_and_query: &str) -> String {
        format!(
            "{}/{}",
            self.cdn_base_url.trim_end_matches('/'),
            path_and_query.trim_start_matches('/')
        )
    }

    async fn post_json<T>(
        &self,
        endpoint: &str,
        payload: Value,
        timeout_ms: u64,
    ) -> Result<T, WeixinApiError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let body = request_body(payload)?;
        let response = self
            .client
            .post(self.endpoint_url(endpoint))
            .headers(ilink_headers(self.token.as_deref(), &body)?)
            .timeout(Duration::from_millis(timeout_ms))
            .body(body)
            .send()
            .await
            .map_err(|error| self.network_error(endpoint, error))?;
        decode_response(endpoint, response).await
    }

    async fn get_json<T>(&self, endpoint: &str, timeout_ms: u64) -> Result<T, WeixinApiError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .get(self.endpoint_url(endpoint))
            .headers(ilink_get_headers()?)
            .timeout(Duration::from_millis(timeout_ms))
            .send()
            .await
            .map_err(|error| self.network_error(endpoint, error))?;
        decode_response(endpoint, response).await
    }

    fn network_error(&self, endpoint: &str, error: reqwest::Error) -> WeixinApiError {
        let mut details = vec![error.to_string()];
        let mut source = error.source();
        while let Some(error) = source {
            details.push(error.to_string());
            source = error.source();
        }
        let redacted = self.token.as_deref().map_or_else(
            || details.join(": "),
            |token| details.join(": ").replace(token, "<redacted>"),
        );
        WeixinApiError::Network(format!("{endpoint} request failed: {redacted}"))
    }

    async fn read_bounded_download_body(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<u8>, WeixinApiError> {
        if let Some(limit) = self.max_download_bytes
            && let Some(content_length) = response.content_length()
            && content_length > limit as u64
        {
            return Err(WeixinApiError::FileTooLarge {
                limit,
                actual: Some(content_length),
            });
        }
        let mut data = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| self.network_error("cdn/download", error))?;
            if let Some(limit) = self.max_download_bytes
                && data.len().saturating_add(chunk.len()) > limit
            {
                return Err(WeixinApiError::FileTooLarge {
                    limit,
                    actual: None,
                });
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
}

impl WeixinApi for ReqwestWeixinApi {
    fn get_updates<'a>(
        &'a self,
        sync_buf: &'a str,
        timeout_ms: u64,
    ) -> WeixinApiFuture<'a, WeixinUpdatesResponse> {
        Box::pin(async move {
            self.post_json(
                EP_GET_UPDATES,
                json!({ "get_updates_buf": sync_buf }),
                timeout_ms,
            )
            .await
        })
    }

    fn send_message<'a>(
        &'a self,
        request: WeixinSendMessageRequest,
    ) -> WeixinApiFuture<'a, WeixinSendMessageResponse> {
        Box::pin(async move {
            if request.msg.item_list.is_empty() {
                return Err(WeixinApiError::InvalidRequest(
                    "sendmessage item_list must not be empty".into(),
                ));
            }
            self.post_json(
                EP_SEND_MESSAGE,
                json!({ "msg": request.msg }),
                API_TIMEOUT_MS,
            )
            .await
        })
    }

    fn send_typing<'a>(
        &'a self,
        request: WeixinSendTypingRequest,
    ) -> WeixinApiFuture<'a, WeixinRawResponse> {
        Box::pin(async move {
            self.post_json(EP_SEND_TYPING, json!(request), CONFIG_TIMEOUT_MS)
                .await
        })
    }

    fn get_config<'a>(
        &'a self,
        request: WeixinGetConfigRequest,
    ) -> WeixinApiFuture<'a, WeixinConfigResponse> {
        Box::pin(async move {
            self.post_json(EP_GET_CONFIG, json!(request), CONFIG_TIMEOUT_MS)
                .await
        })
    }

    fn get_upload_url<'a>(
        &'a self,
        request: WeixinGetUploadUrlRequest,
    ) -> WeixinApiFuture<'a, WeixinUploadUrlResponse> {
        Box::pin(async move {
            self.post_json(EP_GET_UPLOAD_URL, json!(request), API_TIMEOUT_MS)
                .await
        })
    }

    fn upload_ciphertext<'a>(
        &'a self,
        upload_url: &'a str,
        ciphertext: Vec<u8>,
    ) -> WeixinApiFuture<'a, String> {
        Box::pin(async move {
            let response = self
                .client
                .post(upload_url)
                .header(CONTENT_TYPE, "application/octet-stream")
                .timeout(Duration::from_secs(120))
                .body(ciphertext)
                .send()
                .await
                .map_err(|error| self.network_error("cdn/upload", error))?;
            if !response.status().is_success() {
                let status = response.status();
                let raw = response.text().await.unwrap_or_default();
                return Err(WeixinApiError::Http {
                    endpoint: "cdn/upload".into(),
                    status: status.as_u16(),
                    body: truncate_body(&raw),
                });
            }
            let encrypted_param = response
                .headers()
                .get("x-encrypted-param")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
                .ok_or_else(|| {
                    WeixinApiError::Cdn("CDN upload missing x-encrypted-param header".into())
                })?;
            Ok(encrypted_param)
        })
    }

    fn download_bytes<'a>(&'a self, url: &'a str) -> WeixinApiFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let response = self
                .client
                .get(url)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .map_err(|error| self.network_error("cdn/download", error))?;
            if !response.status().is_success() {
                let status = response.status();
                let raw = response.text().await.unwrap_or_default();
                return Err(WeixinApiError::Http {
                    endpoint: "cdn/download".into(),
                    status: status.as_u16(),
                    body: truncate_body(&raw),
                });
            }
            self.read_bounded_download_body(response).await
        })
    }

    fn get_bot_qrcode<'a>(&'a self, bot_type: &'a str) -> WeixinApiFuture<'a, WeixinQrCode> {
        Box::pin(async move {
            let endpoint = format!("{EP_GET_BOT_QR}?bot_type={bot_type}");
            self.get_json(&endpoint, QR_TIMEOUT_MS).await
        })
    }

    fn get_qrcode_status<'a>(&'a self, qrcode: &'a str) -> WeixinApiFuture<'a, WeixinQrStatus> {
        Box::pin(async move {
            let endpoint = format!("{EP_GET_QR_STATUS}?qrcode={qrcode}");
            self.get_json(&endpoint, QR_TIMEOUT_MS).await
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinRawResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinUpdatesResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub get_updates_buf: String,
    #[serde(default)]
    pub longpolling_timeout_ms: Option<u64>,
    #[serde(default)]
    pub msgs: Vec<WeixinMessage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinMessage {
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub message_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub from_user_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub to_user_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub room_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub chat_room_id: Option<String>,
    #[serde(default)]
    pub msg_type: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub context_token: Option<String>,
    #[serde(default)]
    pub item_list: Vec<WeixinMessageItem>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinMessageItem {
    #[serde(rename = "type")]
    pub kind: i64,
    #[serde(default)]
    pub text_item: Option<WeixinTextItem>,
    #[serde(default)]
    pub image_item: Option<WeixinImageItem>,
    #[serde(default)]
    pub voice_item: Option<WeixinVoiceItem>,
    #[serde(default)]
    pub file_item: Option<WeixinFileItem>,
    #[serde(default)]
    pub video_item: Option<WeixinVideoItem>,
    #[serde(default)]
    pub ref_msg: Option<WeixinRefMessage>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinTextItem {
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinMediaRef {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_or_number",
        skip_serializing_if = "Option::is_none"
    )]
    pub encrypt_query_param: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_or_number",
        skip_serializing_if = "Option::is_none"
    )]
    pub aes_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_optional_i64_or_string")]
    pub encrypt_type: Option<i64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string_or_number",
        skip_serializing_if = "Option::is_none"
    )]
    pub full_url: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinImageItem {
    #[serde(default)]
    pub media: Option<WeixinMediaRef>,
    #[serde(default)]
    pub aeskey: Option<String>,
    #[serde(default)]
    pub mid_size: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinVoiceItem {
    #[serde(default)]
    pub media: Option<WeixinMediaRef>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinFileItem {
    #[serde(default)]
    pub media: Option<WeixinMediaRef>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub file_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub len: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinVideoItem {
    #[serde(default)]
    pub media: Option<WeixinMediaRef>,
    #[serde(default)]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub play_length: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinRefMessage {
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub title: Option<String>,
    #[serde(default)]
    pub message_item: Option<Box<WeixinMessageItem>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinSendMessageRequest {
    pub msg: WeixinOutboundMessage,
}

impl WeixinSendMessageRequest {
    pub fn text(
        to_user_id: impl Into<String>,
        text: impl Into<String>,
        context_token: Option<String>,
    ) -> Self {
        Self {
            msg: WeixinOutboundMessage::new(
                to_user_id,
                vec![WeixinOutboundItem::text(text)],
                context_token,
            ),
        }
    }

    pub fn item(
        to_user_id: impl Into<String>,
        item: WeixinOutboundItem,
        context_token: Option<String>,
    ) -> Self {
        Self {
            msg: WeixinOutboundMessage::new(to_user_id, vec![item], context_token),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinOutboundMessage {
    pub from_user_id: String,
    pub to_user_id: String,
    pub client_id: String,
    pub message_type: i64,
    pub message_state: i64,
    pub item_list: Vec<WeixinOutboundItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

impl WeixinOutboundMessage {
    pub fn new(
        to_user_id: impl Into<String>,
        item_list: Vec<WeixinOutboundItem>,
        context_token: Option<String>,
    ) -> Self {
        Self {
            from_user_id: String::new(),
            to_user_id: to_user_id.into(),
            client_id: format!("noloong-weixin-{}", Uuid::new_v4().simple()),
            message_type: MSG_TYPE_BOT,
            message_state: MSG_STATE_FINISH,
            item_list,
            context_token,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinOutboundItem {
    #[serde(rename = "type")]
    pub kind: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_item: Option<WeixinTextItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_item: Option<WeixinImageItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_item: Option<WeixinFileItem>,
}

impl WeixinOutboundItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ITEM_TEXT,
            text_item: Some(WeixinTextItem { text: text.into() }),
            image_item: None,
            file_item: None,
        }
    }

    pub fn image(encrypt_query_param: String, aes_key: String, ciphertext_size: u64) -> Self {
        Self {
            kind: ITEM_IMAGE,
            text_item: None,
            image_item: Some(WeixinImageItem {
                media: Some(WeixinMediaRef {
                    encrypt_query_param: Some(encrypt_query_param),
                    aes_key: Some(aes_key),
                    encrypt_type: Some(MEDIA_ENCRYPT_TYPE_AES),
                    full_url: None,
                    extra: Map::new(),
                }),
                aeskey: None,
                mid_size: Some(ciphertext_size),
            }),
            file_item: None,
        }
    }

    pub fn file(
        encrypt_query_param: String,
        aes_key: String,
        file_name: String,
        plaintext_size: u64,
    ) -> Self {
        Self {
            kind: ITEM_FILE,
            text_item: None,
            image_item: None,
            file_item: Some(WeixinFileItem {
                media: Some(WeixinMediaRef {
                    encrypt_query_param: Some(encrypt_query_param),
                    aes_key: Some(aes_key),
                    encrypt_type: Some(MEDIA_ENCRYPT_TYPE_AES),
                    full_url: None,
                    extra: Map::new(),
                }),
                file_name: Some(file_name),
                len: Some(plaintext_size.to_string()),
            }),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinSendMessageResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub message_id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinSendTypingRequest {
    pub ilink_user_id: String,
    pub typing_ticket: String,
    pub status: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinGetConfigRequest {
    pub ilink_user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinConfigResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub typing_ticket: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinGetUploadUrlRequest {
    pub filekey: String,
    pub media_type: i64,
    pub to_user_id: String,
    pub rawsize: u64,
    pub rawfilemd5: String,
    pub filesize: u64,
    #[serde(rename = "aeskey")]
    pub aes_key_hex: String,
    pub no_need_thumb: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinUploadUrlResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub upload_param: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub upload_full_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub filekey: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl WeixinUploadUrlResponse {
    pub fn upload_url(
        &self,
        api: &ReqwestWeixinApi,
        filekey: &str,
    ) -> Result<String, WeixinApiError> {
        if let Some(url) = &self.upload_full_url {
            return Ok(url.clone());
        }
        let upload_param = self.upload_param.as_deref().ok_or_else(|| {
            WeixinApiError::Cdn("getuploadurl response missing upload_param/upload_full_url".into())
        })?;
        Ok(api.cdn_url(&format!(
            "upload?encrypted_query_param={}&filekey={}",
            url_escape(upload_param),
            url_escape(filekey)
        )))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinQrCode {
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub qrcode: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub qrcode_img_content: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WeixinQrStatus {
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub status: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub redirect_host: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub ilink_bot_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub bot_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub baseurl: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub ilink_user_id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(deserialize_optional_string_or_number(deserializer)?.unwrap_or_default())
}

fn deserialize_optional_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value.map(coerce_json_scalar_to_string).transpose()
}

fn deserialize_optional_i64_or_string<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    value.map(coerce_json_scalar_to_i64).transpose()
}

fn coerce_json_scalar_to_i64<E>(value: Value) -> Result<i64, E>
where
    E: serde::de::Error,
{
    match value {
        Value::Null => Ok(0),
        Value::Number(value) => value
            .as_i64()
            .ok_or_else(|| E::custom(format!("expected i64-compatible number, got {value}"))),
        Value::String(value) => value
            .parse()
            .map_err(|error| E::custom(format!("expected i64-compatible string: {error}"))),
        other => Err(E::custom(format!(
            "expected i64-compatible scalar, got {other}"
        ))),
    }
}

fn coerce_json_scalar_to_string<E>(value: Value) -> Result<String, E>
where
    E: serde::de::Error,
{
    match value {
        Value::Null => Ok(String::new()),
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        other => Err(E::custom(format!(
            "expected string-compatible scalar, got {other}"
        ))),
    }
}

#[derive(Debug, Error)]
pub enum WeixinApiError {
    #[error("Weixin API request is invalid: {0}")]
    InvalidRequest(String),
    #[error("Weixin network failed: {0}")]
    Network(String),
    #[error("Weixin HTTP failed for {endpoint}: status {status}, body {body}")]
    Http {
        endpoint: String,
        status: u16,
        body: String,
    },
    #[error("Weixin response decode failed for {endpoint}: {source}")]
    Decode {
        endpoint: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "Weixin session expired for {endpoint}: ret={ret:?} errcode={errcode:?} errmsg={errmsg:?}"
    )]
    SessionExpired {
        endpoint: String,
        ret: Option<i64>,
        errcode: Option<i64>,
        errmsg: Option<String>,
    },
    #[error(
        "Weixin rate limited for {endpoint}: ret={ret:?} errcode={errcode:?} errmsg={errmsg:?}"
    )]
    RateLimited {
        endpoint: String,
        ret: Option<i64>,
        errcode: Option<i64>,
        errmsg: Option<String>,
    },
    #[error("Weixin iLink error for {endpoint}: ret={ret:?} errcode={errcode:?} errmsg={errmsg:?}")]
    Ilink {
        endpoint: String,
        ret: Option<i64>,
        errcode: Option<i64>,
        errmsg: Option<String>,
    },
    #[error("Weixin CDN failed: {0}")]
    Cdn(String),
    #[error("Weixin file is too large: limit {limit} bytes, actual {actual:?} bytes")]
    FileTooLarge { limit: usize, actual: Option<u64> },
}

impl WeixinApiError {
    pub fn is_session_expired(&self) -> bool {
        matches!(self, Self::SessionExpired { .. })
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }
}

async fn decode_response<T>(
    endpoint: &str,
    response: reqwest::Response,
) -> Result<T, WeixinApiError>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let raw = response.text().await.map_err(|error| {
        WeixinApiError::Network(format!("{endpoint} response read failed: {error}"))
    })?;
    if !status.is_success() {
        return Err(WeixinApiError::Http {
            endpoint: endpoint.into(),
            status: status.as_u16(),
            body: truncate_body(&raw),
        });
    }
    let value = serde_json::from_str::<Value>(&raw).map_err(|source| WeixinApiError::Decode {
        endpoint: endpoint.into(),
        source,
    })?;
    check_ilink_error(endpoint, &value)?;
    serde_json::from_value(value).map_err(|source| WeixinApiError::Decode {
        endpoint: endpoint.into(),
        source,
    })
}

fn check_ilink_error(endpoint: &str, value: &Value) -> Result<(), WeixinApiError> {
    let ret = value.get("ret").and_then(Value::as_i64);
    let errcode = value.get("errcode").and_then(Value::as_i64);
    let errmsg = value
        .get("errmsg")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if ret.is_none_or(|ret| ret == 0) && errcode.is_none_or(|errcode| errcode == 0) {
        return Ok(());
    }
    if ret == Some(SESSION_EXPIRED_ERRCODE)
        || errcode == Some(SESSION_EXPIRED_ERRCODE)
        || is_stale_session_ret(ret, errcode, errmsg.as_deref())
    {
        return Err(WeixinApiError::SessionExpired {
            endpoint: endpoint.into(),
            ret,
            errcode,
            errmsg,
        });
    }
    if ret == Some(RATE_LIMIT_ERRCODE) || errcode == Some(RATE_LIMIT_ERRCODE) {
        return Err(WeixinApiError::RateLimited {
            endpoint: endpoint.into(),
            ret,
            errcode,
            errmsg,
        });
    }
    Err(WeixinApiError::Ilink {
        endpoint: endpoint.into(),
        ret,
        errcode,
        errmsg,
    })
}

pub fn is_stale_session_ret(ret: Option<i64>, errcode: Option<i64>, errmsg: Option<&str>) -> bool {
    if ret != Some(RATE_LIMIT_ERRCODE) && errcode != Some(RATE_LIMIT_ERRCODE) {
        return false;
    }
    errmsg.is_some_and(|errmsg| errmsg.eq_ignore_ascii_case("unknown error"))
}

fn request_body(payload: Value) -> Result<String, WeixinApiError> {
    let mut object = match payload {
        Value::Object(object) => object,
        _ => {
            return Err(WeixinApiError::InvalidRequest(
                "iLink request payload must be a JSON object".into(),
            ));
        }
    };
    object.insert(
        "base_info".into(),
        json!({ "channel_version": CHANNEL_VERSION }),
    );
    serde_json::to_string(&Value::Object(object))
        .map_err(|error| WeixinApiError::InvalidRequest(error.to_string()))
}

fn ilink_headers(token: Option<&str>, body: &str) -> Result<HeaderMap, WeixinApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "AuthorizationType",
        HeaderValue::from_static("ilink_bot_token"),
    );
    headers.insert("X-WECHAT-UIN", header_value(random_wechat_uin())?);
    headers.insert("iLink-App-Id", HeaderValue::from_static(ILINK_APP_ID));
    headers.insert(
        "iLink-App-ClientVersion",
        header_value(ILINK_APP_CLIENT_VERSION.to_string())?,
    );
    headers.insert(CONTENT_LENGTH, header_value(body.len().to_string())?);
    if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
        headers.insert("Authorization", header_value(format!("Bearer {token}"))?);
    }
    Ok(headers)
}

fn ilink_get_headers() -> Result<HeaderMap, WeixinApiError> {
    let mut headers = HeaderMap::new();
    headers.insert("iLink-App-Id", HeaderValue::from_static(ILINK_APP_ID));
    headers.insert(
        "iLink-App-ClientVersion",
        header_value(ILINK_APP_CLIENT_VERSION.to_string())?,
    );
    Ok(headers)
}

fn header_value(value: String) -> Result<HeaderValue, WeixinApiError> {
    HeaderValue::from_str(&value).map_err(|error| WeixinApiError::InvalidRequest(error.to_string()))
}

fn random_wechat_uin() -> String {
    let uuid = Uuid::new_v4();
    let bytes = uuid.as_bytes();
    let value = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    general_purpose::STANDARD.encode(value.to_string())
}

pub fn build_cdn_download_url(cdn_base_url: &str, encrypted_query_param: &str) -> String {
    format!(
        "{}/download?encrypted_query_param={}",
        cdn_base_url.trim_end_matches('/'),
        url_escape(encrypted_query_param)
    )
}

fn url_escape(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

async fn retry_after_rate_limit<T, F, Fut>(mut op: F) -> Result<T, WeixinApiError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, WeixinApiError>>,
{
    let mut last_error = None;
    for attempt in 0..=1 {
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) if error.is_rate_limited() && attempt == 0 => {
                last_error = Some(error);
                sleep(Duration::from_secs(2)).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("retry loop recorded a rate-limit error"))
}

pub async fn send_with_rate_limit_retry(
    api: &dyn WeixinApi,
    request: WeixinSendMessageRequest,
) -> Result<WeixinSendMessageResponse, WeixinApiError> {
    retry_after_rate_limit(|| api.send_message(request.clone())).await
}

fn truncate_body(body: &str) -> String {
    let max = 240;
    if body.chars().count() <= max {
        return body.into();
    }
    let mut truncated = body.chars().take(max).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::{
        EP_GET_UPDATES, ITEM_TEXT, ReqwestWeixinApi, WeixinApi, WeixinApiError, WeixinQrCode,
        WeixinQrStatus, WeixinSendMessageRequest, WeixinUpdatesResponse, check_ilink_error,
        is_stale_session_ret, request_body,
    };
    use serde_json::json;

    #[test]
    fn request_body_injects_base_info() {
        let body = request_body(json!({"get_updates_buf": "abc"})).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(value["get_updates_buf"], "abc");
        assert_eq!(value["base_info"]["channel_version"], "2.2.0");
    }

    #[test]
    fn stale_unknown_error_is_session_expired() {
        assert!(is_stale_session_ret(Some(-2), None, Some("unknown error")));
        assert!(matches!(
            check_ilink_error(
                EP_GET_UPDATES,
                &json!({"ret": -2, "errmsg": "unknown error"})
            ),
            Err(WeixinApiError::SessionExpired { .. })
        ));
    }

    #[test]
    fn rate_limit_is_not_confused_with_stale_session() {
        assert!(matches!(
            check_ilink_error(
                EP_GET_UPDATES,
                &json!({"errcode": -2, "errmsg": "too fast"})
            ),
            Err(WeixinApiError::RateLimited { .. })
        ));
    }

    #[test]
    fn send_text_request_uses_ilink_shape() {
        let request = WeixinSendMessageRequest::text("user", "hello", Some("ctx".into()));
        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(request.msg.to_user_id, "user");
        assert_eq!(request.msg.context_token.as_deref(), Some("ctx"));
        assert_eq!(request.msg.item_list[0].kind, ITEM_TEXT);
        assert_eq!(value["msg"]["to_user_id"], "user");
        assert_eq!(value["msg"]["context_token"], "ctx");
        assert_eq!(value["msg"]["item_list"][0]["text_item"]["text"], "hello");
        assert!(value["msg"].get("toUserId").is_none());
        assert_eq!(
            request.msg.item_list[0]
                .text_item
                .as_ref()
                .map(|item| item.text.as_str()),
            Some("hello")
        );
    }

    #[tokio::test]
    async fn download_bytes_rejects_body_over_configured_limit() {
        let app = axum::Router::new().route("/file", axum::routing::get(|| async { "abcdef" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let api = ReqwestWeixinApi::new(reqwest::Client::new(), None).with_max_download_bytes(3);

        let error = api
            .download_bytes(&format!("http://{address}/file"))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            WeixinApiError::FileTooLarge {
                limit: 3,
                actual: Some(6)
            }
        ));
    }

    #[test]
    fn qr_response_uses_ilink_snake_case_fields() {
        let qr: WeixinQrCode = serde_json::from_value(json!({
            "qrcode": "token",
            "qrcode_img_content": "https://example.com/liteapp"
        }))
        .unwrap();

        assert_eq!(qr.qrcode, "token");
        assert_eq!(
            qr.qrcode_img_content.as_deref(),
            Some("https://example.com/liteapp")
        );
    }

    #[test]
    fn qr_status_uses_ilink_snake_case_fields() {
        let status: WeixinQrStatus = serde_json::from_value(json!({
            "status": "confirmed",
            "redirect_host": "ilink.example.com",
            "ilink_bot_id": "bot-id",
            "bot_token": "token",
            "baseurl": "https://ilink.example.com",
            "ilink_user_id": "user-id"
        }))
        .unwrap();

        assert_eq!(status.status, "confirmed");
        assert_eq!(status.redirect_host.as_deref(), Some("ilink.example.com"));
        assert_eq!(status.ilink_bot_id.as_deref(), Some("bot-id"));
        assert_eq!(status.bot_token.as_deref(), Some("token"));
        assert_eq!(status.baseurl.as_deref(), Some("https://ilink.example.com"));
        assert_eq!(status.ilink_user_id.as_deref(), Some("user-id"));
    }

    #[test]
    fn updates_response_accepts_numeric_ids_as_strings() {
        let updates: WeixinUpdatesResponse = serde_json::from_value(json!({
            "ret": 0,
            "get_updates_buf": 7461438681403525768_u64,
            "msgs": [{
                "message_id": 7461438681403525768_u64,
                "from_user_id": 6212645712_u64,
                "to_user_id": "bot",
                "msg_type": 1,
                "context_token": 12345,
                "item_list": [{
                    "type": 1,
                    "text_item": {"text": "ping"}
                }]
            }]
        }))
        .unwrap();

        assert_eq!(updates.get_updates_buf, "7461438681403525768");
        let message = &updates.msgs[0];
        assert_eq!(message.message_id.as_deref(), Some("7461438681403525768"));
        assert_eq!(message.from_user_id.as_deref(), Some("6212645712"));
        assert_eq!(message.context_token.as_deref(), Some("12345"));
    }
}
