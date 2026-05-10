pub use crate::polling::TelegramUpdate;
use crate::{network::TelegramNetworkConfig, polling::TelegramApiResponse};
use futures_util::StreamExt;
use reqwest::{
    Client,
    multipart::{Form, Part},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    error::Error as StdError,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};
use thiserror::Error;
use tokio::{io::AsyncWriteExt, time::sleep};

pub type TelegramApiFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TelegramApiError>> + Send + 'a>>;

pub trait TelegramApi: Send + Sync {
    fn get_updates<'a>(
        &'a self,
        offset: Option<i64>,
        timeout_seconds: u64,
    ) -> TelegramApiFuture<'a, Vec<TelegramUpdate>>;

    fn get_file<'a>(&'a self, _file_id: &'a str) -> TelegramApiFuture<'a, TelegramFile> {
        unsupported_api_future("getFile")
    }

    fn download_file<'a>(&'a self, _file_path: &'a str) -> TelegramApiFuture<'a, Vec<u8>> {
        unsupported_api_future("downloadFile")
    }

    fn download_file_to_path<'a>(
        &'a self,
        file_path: &'a str,
        target_path: &'a Path,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let data = self.download_file(file_path).await?;
            tokio::fs::write(target_path, data)
                .await
                .map_err(|error| TelegramApiError::File(error.to_string()))
        })
    }

    fn send_message<'a>(
        &'a self,
        request: TelegramSendMessageRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle>;

    fn edit_message_text<'a>(
        &'a self,
        request: TelegramEditMessageTextRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle>;

    fn delete_message<'a>(
        &'a self,
        _request: TelegramDeleteMessageRequest,
    ) -> TelegramApiFuture<'a, ()> {
        unsupported_api_future("deleteMessage")
    }

    fn send_photo<'a>(
        &'a self,
        _request: TelegramSendPhotoRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        unsupported_api_future("sendPhoto")
    }

    fn send_document<'a>(
        &'a self,
        _request: TelegramSendDocumentRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        unsupported_api_future("sendDocument")
    }

    fn send_audio<'a>(
        &'a self,
        _request: TelegramSendAudioRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        unsupported_api_future("sendAudio")
    }

    fn send_voice<'a>(
        &'a self,
        _request: TelegramSendVoiceRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        unsupported_api_future("sendVoice")
    }

    fn send_video<'a>(
        &'a self,
        _request: TelegramSendVideoRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        unsupported_api_future("sendVideo")
    }

    fn send_chat_action<'a>(
        &'a self,
        _request: TelegramSendChatActionRequest,
    ) -> TelegramApiFuture<'a, ()> {
        unsupported_api_future("sendChatAction")
    }

    fn set_my_commands<'a>(
        &'a self,
        _request: TelegramSetMyCommandsRequest,
    ) -> TelegramApiFuture<'a, ()> {
        unsupported_api_future("setMyCommands")
    }

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
    max_download_bytes: Option<usize>,
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
            max_download_bytes: None,
        }
    }

    pub fn with_max_download_bytes(mut self, max_download_bytes: usize) -> Self {
        self.max_download_bytes = Some(max_download_bytes);
        self
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.base_url.trim_end_matches('/'),
            self.token,
            method
        )
    }

    fn file_url(&self, file_path: &str) -> String {
        format!(
            "{}/file/bot{}/{}",
            self.base_url.trim_end_matches('/'),
            self.token,
            file_path.trim_start_matches('/')
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

    async fn send_json<T>(
        &self,
        method: &str,
        body: &T,
    ) -> Result<reqwest::Response, TelegramApiError>
    where
        T: Serialize + ?Sized,
    {
        let mut retry_after = None;
        let mut last_error = None;
        for _ in 0..=1 {
            if let Some(delay) = retry_after.take() {
                sleep(Duration::from_secs(delay)).await;
            }
            let response = self
                .client
                .post(self.method_url(method))
                .json(body)
                .send()
                .await
                .map_err(|error| self.network_error(method, error))?;
            if response.status().as_u16() != 429 {
                return Ok(response);
            }
            let error = parse_telegram_error_response(response).await?;
            retry_after = error.retry_after_seconds();
            if retry_after.is_none() {
                return Err(error);
            }
            last_error = Some(error);
        }
        Err(last_error.unwrap_or_else(|| {
            TelegramApiError::Network(format!(
                "{method} rate-limit retry did not produce a response"
            ))
        }))
    }

    async fn send_media(
        &self,
        method: &str,
        file_field: &'static str,
        request: TelegramSendMediaRequest,
    ) -> Result<TelegramMessageHandle, TelegramApiError> {
        let response = self
            .send_media_with_rate_limit(method, file_field, request)
            .await?;
        parse_sent_message(response).await
    }

    async fn send_media_with_rate_limit(
        &self,
        method: &str,
        file_field: &'static str,
        request: TelegramSendMediaRequest,
    ) -> Result<reqwest::Response, TelegramApiError> {
        let mut retry_after = None;
        let mut last_error = None;
        for _ in 0..=1 {
            if let Some(delay) = retry_after.take() {
                sleep(Duration::from_secs(delay)).await;
            }
            let response = if let TelegramInputFile::FileId(file_id) = &request.input {
                let body = media_json_body(file_field, file_id, &request)?;
                self.send_json(method, &body).await?
            } else {
                let form = media_multipart_form(file_field, request.clone()).await?;
                self.client
                    .post(self.method_url(method))
                    .multipart(form)
                    .send()
                    .await
                    .map_err(|error| self.network_error(method, error))?
            };
            if response.status().as_u16() != 429 {
                return Ok(response);
            }
            let error = parse_telegram_error_response(response).await?;
            retry_after = error.retry_after_seconds();
            if retry_after.is_none() {
                return Err(error);
            }
            last_error = Some(error);
        }
        Err(last_error.unwrap_or_else(|| {
            TelegramApiError::Network(format!(
                "{method} rate-limit retry did not produce a response"
            ))
        }))
    }

    async fn read_bounded_body(
        &self,
        method: &str,
        response: reqwest::Response,
    ) -> Result<Vec<u8>, TelegramApiError> {
        let status = response.status();
        let max_bytes = self.max_download_bytes;
        if !status.is_success() {
            return parse_file_error_response(status.as_u16(), response).await;
        }
        if let Some(limit) = max_bytes
            && let Some(content_length) = response.content_length()
            && content_length > limit as u64
        {
            return Err(TelegramApiError::FileTooLarge {
                limit,
                actual: Some(content_length),
            });
        }
        let mut data = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| self.network_error(method, error))?;
            if let Some(limit) = max_bytes
                && data.len().saturating_add(chunk.len()) > limit
            {
                return Err(TelegramApiError::FileTooLarge {
                    limit,
                    actual: None,
                });
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }

    async fn write_bounded_body(
        &self,
        method: &str,
        response: reqwest::Response,
        target_path: &Path,
    ) -> Result<(), TelegramApiError> {
        let status = response.status();
        let max_bytes = self.max_download_bytes;
        if !status.is_success() {
            return parse_file_error_response(status.as_u16(), response).await;
        }
        if let Some(limit) = max_bytes
            && let Some(content_length) = response.content_length()
            && content_length > limit as u64
        {
            return Err(TelegramApiError::FileTooLarge {
                limit,
                actual: Some(content_length),
            });
        }
        let mut file = tokio::fs::File::create(target_path)
            .await
            .map_err(|error| TelegramApiError::File(error.to_string()))?;
        let mut written = 0_usize;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| self.network_error(method, error))?;
            if let Some(limit) = max_bytes
                && written.saturating_add(chunk.len()) > limit
            {
                return Err(TelegramApiError::FileTooLarge {
                    limit,
                    actual: None,
                });
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| TelegramApiError::File(error.to_string()))?;
            written += chunk.len();
        }
        file.flush()
            .await
            .map_err(|error| TelegramApiError::File(error.to_string()))
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
            let response = self.send_json("getUpdates", &body).await?;
            parse_telegram_response(response).await
        })
    }

    fn get_file<'a>(&'a self, file_id: &'a str) -> TelegramApiFuture<'a, TelegramFile> {
        Box::pin(async move {
            let response = self
                .send_json("getFile", &serde_json::json!({ "file_id": file_id }))
                .await?;
            parse_telegram_response(response).await
        })
    }

    fn download_file<'a>(&'a self, file_path: &'a str) -> TelegramApiFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let response = self
                .client
                .get(self.file_url(file_path))
                .send()
                .await
                .map_err(|error| self.network_error("downloadFile", error))?;
            self.read_bounded_body("downloadFile", response).await
        })
    }

    fn download_file_to_path<'a>(
        &'a self,
        file_path: &'a str,
        target_path: &'a Path,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self
                .client
                .get(self.file_url(file_path))
                .send()
                .await
                .map_err(|error| self.network_error("downloadFile", error))?;
            self.write_bounded_body("downloadFile", response, target_path)
                .await
        })
    }

    fn send_message<'a>(
        &'a self,
        request: TelegramSendMessageRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            let response = self.send_json("sendMessage", &request).await?;
            parse_sent_message(response).await
        })
    }

    fn edit_message_text<'a>(
        &'a self,
        request: TelegramEditMessageTextRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            let response = self.send_json("editMessageText", &request).await?;
            parse_sent_message(response).await
        })
    }

    fn delete_message<'a>(
        &'a self,
        request: TelegramDeleteMessageRequest,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self.send_json("deleteMessage", &request).await?;
            parse_telegram_response::<bool>(response).await.map(|_| ())
        })
    }

    fn send_photo<'a>(
        &'a self,
        request: TelegramSendPhotoRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            self.send_media("sendPhoto", "photo", request.into_media_request())
                .await
        })
    }

    fn send_document<'a>(
        &'a self,
        request: TelegramSendDocumentRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            self.send_media("sendDocument", "document", request.into_media_request())
                .await
        })
    }

    fn send_audio<'a>(
        &'a self,
        request: TelegramSendAudioRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            self.send_media("sendAudio", "audio", request.into_media_request())
                .await
        })
    }

    fn send_voice<'a>(
        &'a self,
        request: TelegramSendVoiceRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            self.send_media("sendVoice", "voice", request.into_media_request())
                .await
        })
    }

    fn send_video<'a>(
        &'a self,
        request: TelegramSendVideoRequest,
    ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
        Box::pin(async move {
            self.send_media("sendVideo", "video", request.into_media_request())
                .await
        })
    }

    fn send_chat_action<'a>(
        &'a self,
        request: TelegramSendChatActionRequest,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self.send_json("sendChatAction", &request).await?;
            parse_telegram_response::<bool>(response).await.map(|_| ())
        })
    }

    fn set_my_commands<'a>(
        &'a self,
        request: TelegramSetMyCommandsRequest,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self.send_json("setMyCommands", &request).await?;
            parse_telegram_response::<bool>(response).await.map(|_| ())
        })
    }

    fn answer_callback_query<'a>(
        &'a self,
        callback_query_id: &'a str,
        text: Option<&'a str>,
    ) -> TelegramApiFuture<'a, ()> {
        Box::pin(async move {
            let response = self
                .send_json(
                    "answerCallbackQuery",
                    &serde_json::json!({
                        "callback_query_id": callback_query_id,
                        "text": text,
                    }),
                )
                .await?;
            parse_telegram_response::<bool>(response).await.map(|_| ())
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramSendMessageRequest {
    pub chat_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
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
#[serde(rename_all = "snake_case")]
pub struct TelegramDeleteMessageRequest {
    pub chat_id: i64,
    pub message_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramFile {
    pub file_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_unique_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramInputFile {
    FileId(String),
    Bytes {
        filename: String,
        data: Vec<u8>,
        mime_type: Option<String>,
    },
    Path {
        path: PathBuf,
        filename: Option<String>,
        mime_type: Option<String>,
    },
}

impl TelegramInputFile {
    pub fn file_id(file_id: impl Into<String>) -> Self {
        Self::FileId(file_id.into())
    }

    pub fn bytes(filename: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            filename: filename.into(),
            data: data.into(),
            mime_type: None,
        }
    }

    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path {
            path: path.into(),
            filename: None,
            mime_type: None,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        let mime_type = Some(mime_type.into());
        match &mut self {
            Self::FileId(_) => {}
            Self::Bytes {
                mime_type: current, ..
            }
            | Self::Path {
                mime_type: current, ..
            } => *current = mime_type,
        }
        self
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        if let Self::Path {
            filename: current, ..
        } = &mut self
        {
            *current = Some(filename.into());
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSendPhotoRequest {
    pub chat_id: i64,
    pub photo: TelegramInputFile,
    pub options: TelegramMediaMessageOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSendDocumentRequest {
    pub chat_id: i64,
    pub document: TelegramInputFile,
    pub options: TelegramMediaMessageOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSendAudioRequest {
    pub chat_id: i64,
    pub audio: TelegramInputFile,
    pub options: TelegramMediaMessageOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSendVoiceRequest {
    pub chat_id: i64,
    pub voice: TelegramInputFile,
    pub options: TelegramMediaMessageOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSendVideoRequest {
    pub chat_id: i64,
    pub video: TelegramInputFile,
    pub options: TelegramMediaMessageOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelegramMediaMessageOptions {
    pub message_thread_id: Option<i64>,
    pub caption: Option<String>,
    pub parse_mode: Option<TelegramParseMode>,
    pub reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

macro_rules! impl_media_request {
    ($request:ty, $field:ident) => {
        impl $request {
            fn into_media_request(self) -> TelegramSendMediaRequest {
                TelegramSendMediaRequest {
                    chat_id: self.chat_id,
                    message_thread_id: self.options.message_thread_id,
                    input: self.$field,
                    caption: self.options.caption,
                    parse_mode: self.options.parse_mode,
                    reply_markup: self.options.reply_markup,
                }
            }
        }
    };
}

impl_media_request!(TelegramSendPhotoRequest, photo);
impl_media_request!(TelegramSendDocumentRequest, document);
impl_media_request!(TelegramSendAudioRequest, audio);
impl_media_request!(TelegramSendVoiceRequest, voice);
impl_media_request!(TelegramSendVideoRequest, video);

#[derive(Clone, Debug, PartialEq, Eq)]
struct TelegramSendMediaRequest {
    chat_id: i64,
    message_thread_id: Option<i64>,
    input: TelegramInputFile,
    caption: Option<String>,
    parse_mode: Option<TelegramParseMode>,
    reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramSendChatActionRequest {
    pub chat_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
    pub action: TelegramChatAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatAction {
    Typing,
    UploadPhoto,
    RecordVideo,
    UploadVideo,
    RecordVoice,
    UploadVoice,
    UploadDocument,
    ChooseSticker,
    FindLocation,
    RecordVideoNote,
    UploadVideoNote,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramSetMyCommandsRequest {
    pub commands: Vec<TelegramBotCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramBotCommand {
    pub command: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TelegramParseMode {
    #[serde(rename = "MarkdownV2")]
    MarkdownV2,
}

impl TelegramParseMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::MarkdownV2 => "MarkdownV2",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramInlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<TelegramInlineKeyboardButton>>,
}

impl TelegramInlineKeyboardMarkup {
    pub fn empty() -> Self {
        Self {
            inline_keyboard: Vec::new(),
        }
    }
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
    Api {
        code: i64,
        description: String,
        retry_after: Option<u64>,
    },
    #[error("telegram response decode failed: {0}")]
    Decode(String),
    #[error("telegram local file failed: {0}")]
    File(String),
    #[error("telegram file is too large: limit {limit} bytes, actual {actual:?} bytes")]
    FileTooLarge { limit: usize, actual: Option<u64> },
    #[error("telegram api method is unsupported by this client: {0}")]
    Unsupported(String),
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

    pub fn retry_after_seconds(&self) -> Option<u64> {
        match self {
            Self::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
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
    #[serde(default)]
    parameters: Option<TelegramResponseParameters>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct TelegramResponseParameters {
    retry_after: Option<u64>,
}

async fn parse_sent_message(
    response: reqwest::Response,
) -> Result<TelegramMessageHandle, TelegramApiError> {
    let message = parse_telegram_response::<TelegramSentMessage>(response).await?;
    Ok(TelegramMessageHandle {
        chat_id: message.chat.id,
        message_id: message.message_id,
    })
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
        return Err(parse_telegram_error_bytes(&bytes)?);
    }
    let body = serde_json::from_slice::<TelegramApiResponse<T>>(&bytes)
        .map_err(|error| TelegramApiError::Decode(error.to_string()))?;
    Ok(body.result)
}

async fn parse_file_error_response<T>(
    status_code: u16,
    response: reqwest::Response,
) -> Result<T, TelegramApiError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| TelegramApiError::Network(error.to_string()))?;
    match serde_json::from_slice::<TelegramErrorResponse>(&bytes) {
        Ok(error) => Err(telegram_api_error(error)),
        Err(_) => Err(TelegramApiError::Api {
            code: i64::from(status_code),
            description: String::from_utf8_lossy(&bytes).into_owned(),
            retry_after: None,
        }),
    }
}

async fn parse_telegram_error_response(
    response: reqwest::Response,
) -> Result<TelegramApiError, TelegramApiError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| TelegramApiError::Network(error.to_string()))?;
    parse_telegram_error_bytes(&bytes)
}

fn parse_telegram_error_bytes(bytes: &[u8]) -> Result<TelegramApiError, TelegramApiError> {
    serde_json::from_slice::<TelegramErrorResponse>(bytes)
        .map(telegram_api_error)
        .map_err(|error| TelegramApiError::Decode(error.to_string()))
}

fn telegram_api_error(error: TelegramErrorResponse) -> TelegramApiError {
    TelegramApiError::Api {
        code: error.error_code,
        description: error.description,
        retry_after: error
            .parameters
            .and_then(|parameters| parameters.retry_after),
    }
}

pub(crate) fn unsupported_api_future<'a, T>(method: &'static str) -> TelegramApiFuture<'a, T> {
    Box::pin(async move { Err(TelegramApiError::Unsupported(method.into())) })
}

fn media_json_body(
    file_field: &'static str,
    file_id: &str,
    request: &TelegramSendMediaRequest,
) -> Result<Value, TelegramApiError> {
    let mut body = Map::new();
    body.insert("chat_id".into(), request.chat_id.into());
    insert_optional_i64(&mut body, "message_thread_id", request.message_thread_id);
    body.insert(file_field.into(), file_id.into());
    insert_optional_string(&mut body, "caption", request.caption.clone());
    if let Some(parse_mode) = &request.parse_mode {
        body.insert("parse_mode".into(), parse_mode.as_str().into());
    }
    if let Some(reply_markup) = &request.reply_markup {
        body.insert("reply_markup".into(), serialize_value(reply_markup)?);
    }
    Ok(Value::Object(body))
}

async fn media_multipart_form(
    file_field: &'static str,
    request: TelegramSendMediaRequest,
) -> Result<Form, TelegramApiError> {
    let mut form = Form::new().text("chat_id", request.chat_id.to_string());
    if let Some(message_thread_id) = request.message_thread_id {
        form = form.text("message_thread_id", message_thread_id.to_string());
    }
    if let Some(caption) = request.caption {
        form = form.text("caption", caption);
    }
    if let Some(parse_mode) = request.parse_mode {
        form = form.text("parse_mode", parse_mode.as_str());
    }
    if let Some(reply_markup) = request.reply_markup {
        form = form.text("reply_markup", serialize_string(&reply_markup)?);
    }
    Ok(form.part(file_field, request.input.into_part().await?))
}

impl TelegramInputFile {
    async fn into_part(self) -> Result<Part, TelegramApiError> {
        match self {
            Self::FileId(file_id) => Ok(Part::text(file_id)),
            Self::Bytes {
                filename,
                data,
                mime_type,
            } => with_optional_mime(Part::bytes(data).file_name(filename), mime_type),
            Self::Path {
                path,
                filename,
                mime_type,
            } => {
                let mut part = Part::file(&path)
                    .await
                    .map_err(|error| TelegramApiError::File(file_error(&path, error)))?;
                if let Some(filename) = filename {
                    part = part.file_name(filename);
                }
                with_optional_mime(part, mime_type)
            }
        }
    }
}

fn with_optional_mime(part: Part, mime_type: Option<String>) -> Result<Part, TelegramApiError> {
    match mime_type {
        Some(mime_type) => part
            .mime_str(&mime_type)
            .map_err(|error| TelegramApiError::File(error.to_string())),
        None => Ok(part),
    }
}

fn file_error(path: &Path, error: std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

fn serialize_value<T>(value: &T) -> Result<Value, TelegramApiError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| TelegramApiError::Decode(error.to_string()))
}

fn serialize_string<T>(value: &T) -> Result<String, TelegramApiError>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| TelegramApiError::Decode(error.to_string()))
}

fn insert_optional_i64(body: &mut Map<String, Value>, key: &'static str, value: Option<i64>) {
    if let Some(value) = value {
        body.insert(key.into(), value.into());
    }
}

fn insert_optional_string(body: &mut Map<String, Value>, key: &'static str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(key.into(), value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramApiError, TelegramApiResponse, TelegramChatAction, TelegramEditMessageTextRequest,
        TelegramInlineKeyboardButton, TelegramInlineKeyboardMarkup, TelegramInputFile,
        TelegramParseMode, TelegramSendChatActionRequest, TelegramSendMediaRequest,
        TelegramSendMessageRequest, TelegramSentMessage, media_json_body,
        parse_telegram_error_bytes,
    };
    use serde_json::json;

    #[test]
    fn send_message_request_serializes_bot_api_snake_case() {
        let request = TelegramSendMessageRequest {
            chat_id: 42,
            message_thread_id: Some(5),
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
                "message_thread_id": 5,
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
    fn chat_action_request_serializes_bot_api_action() {
        let request = TelegramSendChatActionRequest {
            chat_id: 42,
            message_thread_id: Some(9),
            action: TelegramChatAction::UploadDocument,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "chat_id": 42,
                "message_thread_id": 9,
                "action": "upload_document"
            })
        );
    }

    #[test]
    fn media_file_id_request_serializes_without_multipart() {
        let request = TelegramSendMediaRequest {
            chat_id: 42,
            message_thread_id: Some(9),
            input: TelegramInputFile::file_id("file-1"),
            caption: Some("report".into()),
            parse_mode: Some(TelegramParseMode::MarkdownV2),
            reply_markup: None,
        };

        assert_eq!(
            media_json_body("document", "file-1", &request).unwrap(),
            json!({
                "chat_id": 42,
                "message_thread_id": 9,
                "document": "file-1",
                "caption": "report",
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

    #[test]
    fn api_error_extracts_retry_after_parameter() {
        let error = parse_telegram_error_bytes(
            br#"{
                "ok": false,
                "error_code": 429,
                "description": "Too Many Requests: retry after 7",
                "parameters": {"retry_after": 7}
            }"#,
        )
        .unwrap();

        assert_eq!(
            error,
            TelegramApiError::Api {
                code: 429,
                description: "Too Many Requests: retry after 7".into(),
                retry_after: Some(7),
            }
        );
        assert_eq!(error.retry_after_seconds(), Some(7));
    }
}
