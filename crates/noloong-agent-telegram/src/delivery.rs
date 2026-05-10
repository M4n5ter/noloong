use crate::{
    render::{render_agent_message_text, render_content_block_text, render_markdown_v2},
    telegram_api::{
        TelegramApi, TelegramApiError, TelegramChatAction, TelegramEditMessageTextRequest,
        TelegramInlineKeyboardMarkup, TelegramInputFile, TelegramMediaMessageOptions,
        TelegramMessageHandle, TelegramParseMode, TelegramSendAudioRequest,
        TelegramSendChatActionRequest, TelegramSendDocumentRequest, TelegramSendMessageRequest,
        TelegramSendPhotoRequest, TelegramSendVideoRequest, TelegramSendVoiceRequest,
    },
    text::{split_telegram_text_with_continuation, telegram_utf16_units},
};
use base64::{Engine as _, engine::general_purpose};
use noloong_agent_core::{
    AgentMessage, ContentBlock, MediaBlock, MediaEncoding, MediaKind, MediaSource,
};
use std::{path::PathBuf, sync::Arc};
use thiserror::Error;
use url::Url;

const TELEGRAM_CAPTION_LIMIT_UTF16_UNITS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelegramMessageTarget {
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
}

impl TelegramMessageTarget {
    pub fn new(chat_id: i64, message_thread_id: Option<i64>) -> Self {
        Self {
            chat_id,
            message_thread_id,
        }
    }

    pub fn chat(chat_id: i64) -> Self {
        Self::new(chat_id, None)
    }
}

#[derive(Clone)]
pub struct TelegramDelivery {
    api: Arc<dyn TelegramApi>,
    max_message_units: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TelegramPreviewMessage {
    pub message_id: i64,
    pub text: String,
}

impl TelegramDelivery {
    pub fn new(api: Arc<dyn TelegramApi>, max_message_units: usize) -> Self {
        Self {
            api,
            max_message_units,
        }
    }

    pub async fn send_agent_message(
        &self,
        target: TelegramMessageTarget,
        message: &AgentMessage,
    ) -> TelegramDeliveryResult<Vec<TelegramMessageHandle>> {
        if !agent_message_has_media(message) {
            return self
                .send_text(target, &render_agent_message_text(message), None)
                .await;
        }

        self.send_media_upload_action_for_message(target, message)
            .await;
        let mut sent = Vec::new();
        let mut pending_text = String::new();
        for block in &message.content {
            match block {
                ContentBlock::Media { media } => {
                    let caption = match take_caption_or_flush(&mut pending_text) {
                        PendingCaption::None => None,
                        PendingCaption::Caption(caption) => Some(caption),
                        PendingCaption::Flush(text) => {
                            sent.extend(self.send_text(target, &text, None).await?);
                            None
                        }
                    };
                    sent.push(self.send_media_or_fallback(target, media, caption).await?);
                }
                _ => push_pending_text(&mut pending_text, block),
            }
        }
        if !pending_text.trim().is_empty() {
            sent.extend(self.send_text(target, pending_text.trim(), None).await?);
        }
        Ok(sent)
    }

    pub(crate) async fn send_agent_final_message(
        &self,
        target: TelegramMessageTarget,
        preview: Option<TelegramPreviewMessage>,
        message: &AgentMessage,
    ) -> TelegramDeliveryResult<()> {
        let Some(preview) = preview else {
            self.send_agent_message(target, message).await?;
            return Ok(());
        };

        if !agent_message_has_media(message) {
            let text = render_agent_message_text(message);
            if self
                .edit_text(target, preview.message_id, &text, None)
                .await
                .is_err()
            {
                self.send_text(target, &text, None).await?;
            }
            return Ok(());
        }

        let text = render_agent_message_without_media(message);
        let should_edit_preview = !text.trim().is_empty() && text != preview.text;
        if should_edit_preview
            && self
                .edit_text(target, preview.message_id, &text, None)
                .await
                .is_err()
        {
            self.send_agent_message(target, message).await?;
            return Ok(());
        }
        self.send_media_blocks(target, message).await?;
        Ok(())
    }

    pub async fn send_text(
        &self,
        target: TelegramMessageTarget,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<Vec<TelegramMessageHandle>> {
        let chunks = split_telegram_text_with_continuation(text, self.max_message_units);
        let mut sent = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            sent.push(
                self.send_one_text(target, &chunk, reply_markup.clone())
                    .await?,
            );
        }
        Ok(sent)
    }

    pub async fn send_chat_action(
        &self,
        target: TelegramMessageTarget,
        action: TelegramChatAction,
    ) -> TelegramDeliveryResult<()> {
        self.api
            .send_chat_action(TelegramSendChatActionRequest {
                chat_id: target.chat_id,
                message_thread_id: target.message_thread_id,
                action,
            })
            .await
            .map_err(TelegramDeliveryError::Api)
    }

    pub async fn send_chat_action_best_effort(
        &self,
        target: TelegramMessageTarget,
        action: TelegramChatAction,
    ) {
        let _ = self.send_chat_action(target, action).await;
    }

    pub async fn edit_text(
        &self,
        target: TelegramMessageTarget,
        message_id: i64,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        let rendered = render_markdown_v2(text);
        match self
            .api
            .edit_message_text(TelegramEditMessageTextRequest {
                chat_id: target.chat_id,
                message_id,
                text: rendered,
                parse_mode: Some(TelegramParseMode::MarkdownV2),
                reply_markup: reply_markup.clone(),
            })
            .await
        {
            Ok(message) => Ok(message),
            Err(error) if error.is_message_not_modified() => Ok(TelegramMessageHandle {
                chat_id: target.chat_id,
                message_id,
            }),
            Err(error) if error.is_parse_error() => self
                .api
                .edit_message_text(TelegramEditMessageTextRequest {
                    chat_id: target.chat_id,
                    message_id,
                    text: text.into(),
                    parse_mode: None,
                    reply_markup,
                })
                .await
                .map_err(TelegramDeliveryError::Api),
            Err(error) => Err(TelegramDeliveryError::Api(error)),
        }
    }

    async fn send_one_text(
        &self,
        target: TelegramMessageTarget,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        let rendered = render_markdown_v2(text);
        match self
            .api
            .send_message(TelegramSendMessageRequest {
                chat_id: target.chat_id,
                message_thread_id: target.message_thread_id,
                text: rendered,
                parse_mode: Some(TelegramParseMode::MarkdownV2),
                reply_markup: reply_markup.clone(),
            })
            .await
        {
            Ok(message) => Ok(message),
            Err(error) if error.is_parse_error() => self
                .api
                .send_message(TelegramSendMessageRequest {
                    chat_id: target.chat_id,
                    message_thread_id: target.message_thread_id,
                    text: text.into(),
                    parse_mode: None,
                    reply_markup,
                })
                .await
                .map_err(TelegramDeliveryError::Api),
            Err(error) => Err(TelegramDeliveryError::Api(error)),
        }
    }

    async fn send_media_or_fallback(
        &self,
        target: TelegramMessageTarget,
        media: &MediaBlock,
        caption: Option<String>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        match self.send_media_native(target, media, caption.clone()).await {
            Ok(message) => Ok(message),
            Err(error) => {
                let text = media_fallback_text(media, Some(error.to_string()), caption);
                self.send_one_text(target, &text, None).await
            }
        }
    }

    async fn send_media_blocks(
        &self,
        target: TelegramMessageTarget,
        message: &AgentMessage,
    ) -> TelegramDeliveryResult<Vec<TelegramMessageHandle>> {
        self.send_media_upload_action_for_message(target, message)
            .await;
        let mut sent = Vec::new();
        for block in &message.content {
            let ContentBlock::Media { media } = block else {
                continue;
            };
            sent.push(self.send_media_or_fallback(target, media, None).await?);
        }
        Ok(sent)
    }

    async fn send_media_upload_action_for_message(
        &self,
        target: TelegramMessageTarget,
        message: &AgentMessage,
    ) {
        let Some(action) = message.content.iter().find_map(|block| match block {
            ContentBlock::Media { media }
                if !matches!(media.source, MediaSource::Provider { .. }) =>
            {
                Some(TelegramNativeMediaKind::for_media_kind(&media.kind).action())
            }
            _ => None,
        }) else {
            return;
        };
        self.send_chat_action_best_effort(target, action).await;
    }

    async fn send_media_native(
        &self,
        target: TelegramMessageTarget,
        media: &MediaBlock,
        caption: Option<String>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        let input = telegram_input_file(media)?;
        let options = TelegramMediaMessageOptions {
            message_thread_id: target.message_thread_id,
            caption,
            parse_mode: None,
            reply_markup: None,
        };
        match TelegramNativeMediaKind::for_media_kind(&media.kind) {
            TelegramNativeMediaKind::Photo => {
                self.api
                    .send_photo(TelegramSendPhotoRequest {
                        chat_id: target.chat_id,
                        photo: input,
                        options,
                    })
                    .await
            }
            TelegramNativeMediaKind::Audio => {
                self.api
                    .send_audio(TelegramSendAudioRequest {
                        chat_id: target.chat_id,
                        audio: input,
                        options,
                    })
                    .await
            }
            TelegramNativeMediaKind::Video => {
                self.api
                    .send_video(TelegramSendVideoRequest {
                        chat_id: target.chat_id,
                        video: input,
                        options,
                    })
                    .await
            }
            TelegramNativeMediaKind::Voice => {
                self.api
                    .send_voice(TelegramSendVoiceRequest {
                        chat_id: target.chat_id,
                        voice: input,
                        options,
                    })
                    .await
            }
            TelegramNativeMediaKind::Document => {
                self.api
                    .send_document(TelegramSendDocumentRequest {
                        chat_id: target.chat_id,
                        document: input,
                        options,
                    })
                    .await
            }
        }
        .map_err(TelegramDeliveryError::Api)
    }
}

pub type TelegramDeliveryResult<T> = Result<T, TelegramDeliveryError>;

#[derive(Debug, Error)]
pub enum TelegramDeliveryError {
    #[error("telegram api request failed: {0}")]
    Api(#[from] TelegramApiError),
    #[error("telegram media source is unsupported: {0}")]
    UnsupportedMediaSource(#[from] UnsupportedTelegramMediaSource),
    #[error("telegram media base64 decode failed: {0}")]
    MediaDecode(#[from] base64::DecodeError),
    #[error("telegram media file URI is invalid: {0}")]
    InvalidFileUri(#[from] TelegramFileUriError),
}

#[derive(Debug, Error)]
pub enum UnsupportedTelegramMediaSource {
    #[error("inline media encoding {0}")]
    InlineEncoding(String),
    #[error("provider media {provider_id}/{id}")]
    Provider { provider_id: String, id: String },
    #[error("uri scheme {0}")]
    UriScheme(String),
}

#[derive(Debug, Error)]
pub enum TelegramFileUriError {
    #[error("{uri}: {source}")]
    Parse {
        uri: String,
        #[source]
        source: url::ParseError,
    },
    #[error("{0}")]
    NotFilePath(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TelegramNativeMediaKind {
    Photo,
    Audio,
    Video,
    Voice,
    Document,
}

impl TelegramNativeMediaKind {
    fn for_media_kind(kind: &MediaKind) -> Self {
        match kind {
            MediaKind::Image => Self::Photo,
            MediaKind::Audio => Self::Audio,
            MediaKind::Video => Self::Video,
            MediaKind::Custom(kind) if kind == "voice" => Self::Voice,
            MediaKind::File | MediaKind::Custom(_) => Self::Document,
        }
    }

    fn action(self) -> TelegramChatAction {
        match self {
            Self::Photo => TelegramChatAction::UploadPhoto,
            Self::Audio | Self::Document => TelegramChatAction::UploadDocument,
            Self::Video => TelegramChatAction::UploadVideo,
            Self::Voice => TelegramChatAction::UploadVoice,
        }
    }
}

fn agent_message_has_media(message: &AgentMessage) -> bool {
    message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Media { .. }))
}

fn render_agent_message_without_media(message: &AgentMessage) -> String {
    let mut rendered = String::new();
    for block in &message.content {
        if matches!(block, ContentBlock::Media { .. }) {
            continue;
        }
        let Some(text) = render_content_block_text(block) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(&text);
    }
    rendered
}

fn push_pending_text(pending_text: &mut String, block: &ContentBlock) {
    let Some(text) = render_content_block_text(block) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    if !pending_text.is_empty() {
        pending_text.push('\n');
    }
    pending_text.push_str(&text);
}

enum PendingCaption {
    None,
    Caption(String),
    Flush(String),
}

fn take_caption_or_flush(pending_text: &mut String) -> PendingCaption {
    let text = pending_text.trim();
    if text.is_empty() {
        pending_text.clear();
        return PendingCaption::None;
    }
    let text = text.to_owned();
    pending_text.clear();
    if telegram_utf16_units(&text) > TELEGRAM_CAPTION_LIMIT_UTF16_UNITS {
        PendingCaption::Flush(text)
    } else {
        PendingCaption::Caption(text)
    }
}

fn telegram_input_file(media: &MediaBlock) -> TelegramDeliveryResult<TelegramInputFile> {
    let mut input = match &media.source {
        MediaSource::Inline { data, encoding } => {
            if *encoding != MediaEncoding::Base64 {
                return Err(UnsupportedTelegramMediaSource::InlineEncoding(
                    encoding.as_str().into(),
                )
                .into());
            }
            let bytes = general_purpose::STANDARD
                .decode(data)
                .map_err(TelegramDeliveryError::MediaDecode)?;
            TelegramInputFile::bytes(media_filename(media), bytes)
        }
        MediaSource::Uri { uri } => TelegramInputFile::path(file_uri_path(uri)?),
        MediaSource::Provider { provider_id, id } => {
            return Err(UnsupportedTelegramMediaSource::Provider {
                provider_id: provider_id.clone(),
                id: id.clone(),
            }
            .into());
        }
    };
    if let Some(mime_type) = &media.mime_type {
        input = input.with_mime_type(mime_type.clone());
    }
    if let MediaSource::Uri { .. } = &media.source
        && let Some(name) = &media.name
    {
        input = input.with_filename(name.clone());
    }
    Ok(input)
}

fn media_filename(media: &MediaBlock) -> String {
    media.name.clone().unwrap_or_else(|| match &media.kind {
        MediaKind::Image => "media.jpg".into(),
        MediaKind::Audio => "media.audio".into(),
        MediaKind::Video => "media.video".into(),
        MediaKind::File => "media.file".into(),
        MediaKind::Custom(kind) => format!("media.{kind}"),
    })
}

fn file_uri_path(uri: &str) -> TelegramDeliveryResult<PathBuf> {
    let url = Url::parse(uri).map_err(|source| TelegramFileUriError::Parse {
        uri: uri.into(),
        source,
    })?;
    if url.scheme() != "file" {
        return Err(UnsupportedTelegramMediaSource::UriScheme(url.scheme().into()).into());
    }
    url.to_file_path()
        .map_err(|()| TelegramFileUriError::NotFilePath(uri.into()).into())
}

fn media_fallback_text(
    media: &MediaBlock,
    reason: Option<String>,
    caption: Option<String>,
) -> String {
    let mut lines = Vec::new();
    if let Some(caption) = caption.filter(|caption| !caption.trim().is_empty()) {
        lines.push(caption);
    }
    lines.push(format!("Media: {}", media.kind.as_str()));
    if let Some(name) = &media.name {
        lines.push(format!("Name: {name}"));
    }
    if let Some(mime_type) = &media.mime_type {
        lines.push(format!("MIME: {mime_type}"));
    }
    if let Some(reason) = reason {
        lines.push(format!("Fallback reason: {reason}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{TelegramDelivery, TelegramMessageTarget};
    use crate::{
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramChatAction, TelegramEditMessageTextRequest,
            TelegramMessageHandle, TelegramSendAudioRequest, TelegramSendChatActionRequest,
            TelegramSendDocumentRequest, TelegramSendMessageRequest, TelegramSendPhotoRequest,
            TelegramSendVideoRequest, TelegramSendVoiceRequest, TelegramUpdate,
        },
        text::split_telegram_text,
    };
    use noloong_agent_core::{AgentMessage, ContentBlock, MediaBlock, MediaKind};
    use std::{
        future::Future,
        path::PathBuf,
        pin::Pin,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };
    use url::Url;

    #[test]
    fn split_keeps_head_and_tail_chunks() {
        let chunks = split_telegram_text("abc\ndef", 4);
        assert_eq!(chunks, vec!["abc\n", "def"]);
    }

    #[test]
    fn split_long_line_on_char_boundary() {
        let chunks = split_telegram_text("a你b好c", 3);
        assert_eq!(chunks, vec!["a你b", "好c"]);
    }

    #[tokio::test]
    async fn telegram_text_splitting_uses_utf16_limit() {
        let chunks = split_telegram_text("a😀b", 3);
        assert_eq!(chunks, vec!["a😀", "b"]);
    }

    #[tokio::test]
    async fn send_edit_fallbacks_retry_plain_text_after_parse_error() {
        let api = Arc::new(FakeTelegramApi::parse_error_once());
        let delivery = TelegramDelivery::new(api.clone(), 3900);

        let sent = delivery
            .send_text(TelegramMessageTarget::chat(42), "*hello*", None)
            .await
            .unwrap();

        assert_eq!(sent[0].message_id, 1);
        let calls = api
            .sent_calls
            .lock()
            .expect("fake sent calls lock poisoned")
            .clone();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].parse_mode.is_some());
        assert!(calls[1].parse_mode.is_none());
    }

    #[tokio::test]
    async fn send_text_preserves_thread_target() {
        let api = Arc::new(FakeTelegramApi::parse_error_once());
        let delivery = TelegramDelivery::new(api.clone(), 3900);

        delivery
            .send_text(TelegramMessageTarget::new(42, Some(9)), "hello", None)
            .await
            .unwrap();

        let calls = api
            .sent_calls
            .lock()
            .expect("fake sent calls lock poisoned")
            .clone();
        assert_eq!(
            calls.last().and_then(|call| call.message_thread_id),
            Some(9)
        );
    }

    #[tokio::test]
    async fn send_edit_fallbacks_treat_not_modified_as_success() {
        let api = Arc::new(FakeTelegramApi::message_not_modified());
        let delivery = TelegramDelivery::new(api, 3900);

        let edited = delivery
            .edit_text(TelegramMessageTarget::chat(42), 9, "same", None)
            .await
            .unwrap();

        assert_eq!(edited.message_id, 9);
    }

    #[tokio::test]
    async fn send_agent_message_sends_inline_image_as_photo_with_caption() {
        let api = Arc::new(FakeTelegramApi::normal());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut media = MediaBlock::inline_base64(MediaKind::Image, "YWJj");
        media.name = Some("plot.jpg".into());
        media.mime_type = Some("image/jpeg".into());

        let sent = delivery
            .send_agent_message(
                TelegramMessageTarget::new(42, Some(9)),
                &assistant_message(vec![
                    ContentBlock::Text {
                        text: "caption".into(),
                    },
                    ContentBlock::Media { media },
                ]),
            )
            .await
            .unwrap();

        assert_eq!(sent.len(), 1);
        let photo_calls = api.photo_calls.lock().unwrap().clone();
        assert_eq!(photo_calls.len(), 1);
        assert_eq!(photo_calls[0].options.message_thread_id, Some(9));
        assert_eq!(photo_calls[0].options.caption.as_deref(), Some("caption"));
        assert_eq!(
            api.chat_actions.lock().unwrap()[0].action,
            TelegramChatAction::UploadPhoto
        );
    }

    #[tokio::test]
    async fn send_agent_message_flushes_long_text_before_media() {
        let api = Arc::new(FakeTelegramApi::normal());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let text = "a".repeat(1025);
        let media = MediaBlock::inline_base64(MediaKind::Image, "YWJj");

        let sent = delivery
            .send_agent_message(
                TelegramMessageTarget::chat(42),
                &assistant_message(vec![
                    ContentBlock::Text { text: text.clone() },
                    ContentBlock::Media { media },
                ]),
            )
            .await
            .unwrap();

        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].message_id, 1);
        assert_eq!(sent[1].message_id, 2);
        let sent_calls = api.sent_calls.lock().unwrap().clone();
        assert_eq!(sent_calls.len(), 1);
        assert_eq!(sent_calls[0].text, text);
        let photo_calls = api.photo_calls.lock().unwrap().clone();
        assert_eq!(photo_calls.len(), 1);
        assert_eq!(photo_calls[0].options.caption, None);
    }

    #[tokio::test]
    async fn send_agent_message_sends_file_uri_document_from_path() {
        let path = unique_test_file("document.txt");
        std::fs::write(&path, b"hello").unwrap();
        let api = Arc::new(FakeTelegramApi::normal());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut media = MediaBlock::uri(
            MediaKind::File,
            Url::from_file_path(&path).unwrap().to_string(),
        );
        media.name = Some("document.txt".into());
        media.mime_type = Some("text/plain".into());

        delivery
            .send_agent_message(
                TelegramMessageTarget::chat(42),
                &assistant_message(vec![ContentBlock::Media { media }]),
            )
            .await
            .unwrap();

        let document_calls = api.document_calls.lock().unwrap().clone();
        assert_eq!(document_calls.len(), 1);
        assert_eq!(document_calls[0].options.caption, None);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn send_agent_message_routes_media_kinds_to_native_methods() {
        let api = Arc::new(FakeTelegramApi::normal());
        let delivery = TelegramDelivery::new(api.clone(), 3900);

        delivery
            .send_agent_message(
                TelegramMessageTarget::new(42, Some(9)),
                &assistant_message(vec![
                    ContentBlock::Media {
                        media: MediaBlock::inline_base64(MediaKind::Audio, "YWJj"),
                    },
                    ContentBlock::Media {
                        media: MediaBlock::inline_base64(MediaKind::Video, "YWJj"),
                    },
                    ContentBlock::Media {
                        media: MediaBlock::inline_base64(MediaKind::Custom("voice".into()), "YWJj"),
                    },
                    ContentBlock::Media {
                        media: MediaBlock::inline_base64(
                            MediaKind::Custom("artifact".into()),
                            "YWJj",
                        ),
                    },
                ]),
            )
            .await
            .unwrap();

        let audio_calls = api.audio_calls.lock().unwrap().clone();
        let video_calls = api.video_calls.lock().unwrap().clone();
        let voice_calls = api.voice_calls.lock().unwrap().clone();
        let document_calls = api.document_calls.lock().unwrap().clone();
        assert_eq!(audio_calls.len(), 1);
        assert_eq!(video_calls.len(), 1);
        assert_eq!(voice_calls.len(), 1);
        assert_eq!(document_calls.len(), 1);
        assert_eq!(audio_calls[0].options.message_thread_id, Some(9));
        assert_eq!(video_calls[0].options.message_thread_id, Some(9));
        assert_eq!(voice_calls[0].options.message_thread_id, Some(9));
        assert_eq!(document_calls[0].options.message_thread_id, Some(9));
        let actions = api.chat_actions.lock().unwrap().clone();
        assert_eq!(
            actions
                .iter()
                .map(|request| request.action.clone())
                .collect::<Vec<_>>(),
            vec![TelegramChatAction::UploadDocument]
        );
    }

    #[tokio::test]
    async fn send_agent_message_fallbacks_provider_media_to_text() {
        let api = Arc::new(FakeTelegramApi::normal());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut media = MediaBlock::provider(MediaKind::Image, "provider", "image-1");
        media.name = Some("remote-image".into());

        delivery
            .send_agent_message(
                TelegramMessageTarget::chat(42),
                &assistant_message(vec![ContentBlock::Media { media }]),
            )
            .await
            .unwrap();

        assert!(api.photo_calls.lock().unwrap().is_empty());
        assert!(api.chat_actions.lock().unwrap().is_empty());
        let sent_calls = api.sent_calls.lock().unwrap().clone();
        assert_eq!(sent_calls.len(), 1);
        assert!(sent_calls[0].text.contains("provider media"));
    }

    #[tokio::test]
    async fn send_agent_message_fallbacks_failed_media_send_to_text() {
        let api = Arc::new(FakeTelegramApi::media_error());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let media = MediaBlock::inline_base64(MediaKind::Image, "YWJj");

        delivery
            .send_agent_message(
                TelegramMessageTarget::chat(42),
                &assistant_message(vec![ContentBlock::Media { media }]),
            )
            .await
            .unwrap();

        assert_eq!(api.photo_calls.lock().unwrap().len(), 1);
        let sent_calls = api.sent_calls.lock().unwrap().clone();
        assert_eq!(sent_calls.len(), 1);
        assert!(sent_calls[0].text.contains("Fallback reason"));
    }

    struct FakeTelegramApi {
        sent_calls: Mutex<Vec<TelegramSendMessageRequest>>,
        edited_calls: Mutex<Vec<TelegramEditMessageTextRequest>>,
        photo_calls: Mutex<Vec<TelegramSendPhotoRequest>>,
        document_calls: Mutex<Vec<TelegramSendDocumentRequest>>,
        audio_calls: Mutex<Vec<TelegramSendAudioRequest>>,
        video_calls: Mutex<Vec<TelegramSendVideoRequest>>,
        voice_calls: Mutex<Vec<TelegramSendVoiceRequest>>,
        chat_actions: Mutex<Vec<TelegramSendChatActionRequest>>,
        mode: FakeMode,
    }

    impl FakeTelegramApi {
        fn normal() -> Self {
            Self::new(FakeMode::Normal)
        }

        fn parse_error_once() -> Self {
            Self::new(FakeMode::ParseErrorOnce)
        }

        fn message_not_modified() -> Self {
            Self::new(FakeMode::MessageNotModified)
        }

        fn media_error() -> Self {
            Self::new(FakeMode::MediaError)
        }

        fn new(mode: FakeMode) -> Self {
            Self {
                sent_calls: Mutex::new(Vec::new()),
                edited_calls: Mutex::new(Vec::new()),
                photo_calls: Mutex::new(Vec::new()),
                document_calls: Mutex::new(Vec::new()),
                audio_calls: Mutex::new(Vec::new()),
                video_calls: Mutex::new(Vec::new()),
                voice_calls: Mutex::new(Vec::new()),
                chat_actions: Mutex::new(Vec::new()),
                mode,
            }
        }
    }

    #[derive(Clone, Copy)]
    enum FakeMode {
        Normal,
        ParseErrorOnce,
        MessageNotModified,
        MediaError,
    }

    impl TelegramApi for FakeTelegramApi {
        fn get_updates<'a>(
            &'a self,
            _offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn send_message<'a>(
            &'a self,
            request: TelegramSendMessageRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut calls = self
                    .sent_calls
                    .lock()
                    .expect("fake sent calls lock poisoned");
                calls.push(request.clone());
                if matches!(self.mode, FakeMode::ParseErrorOnce) && calls.len() == 1 {
                    return Err(TelegramApiError::Api {
                        code: 400,
                        description: "can't parse entities".into(),
                    });
                }
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: 1,
                })
            })
        }

        fn edit_message_text<'a>(
            &'a self,
            request: TelegramEditMessageTextRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.edited_calls
                    .lock()
                    .expect("fake edited calls lock poisoned")
                    .push(request.clone());
                if matches!(self.mode, FakeMode::MessageNotModified) {
                    return Err(TelegramApiError::Api {
                        code: 400,
                        description: "message is not modified".into(),
                    });
                }
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: request.message_id,
                })
            })
        }

        fn answer_callback_query<'a>(
            &'a self,
            _callback_query_id: &'a str,
            _text: Option<&'a str>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }

        fn send_chat_action<'a>(
            &'a self,
            request: TelegramSendChatActionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async move {
                self.chat_actions.lock().unwrap().push(request);
                Ok(())
            })
        }

        fn send_photo<'a>(
            &'a self,
            request: TelegramSendPhotoRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.photo_calls.lock().unwrap().push(request.clone());
                self.media_result(request.chat_id)
            })
        }

        fn send_document<'a>(
            &'a self,
            request: TelegramSendDocumentRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.document_calls.lock().unwrap().push(request.clone());
                self.media_result(request.chat_id)
            })
        }

        fn send_audio<'a>(
            &'a self,
            request: TelegramSendAudioRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.audio_calls.lock().unwrap().push(request.clone());
                self.media_result(request.chat_id)
            })
        }

        fn send_video<'a>(
            &'a self,
            request: TelegramSendVideoRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.video_calls.lock().unwrap().push(request.clone());
                self.media_result(request.chat_id)
            })
        }

        fn send_voice<'a>(
            &'a self,
            request: TelegramSendVoiceRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.voice_calls.lock().unwrap().push(request.clone());
                self.media_result(request.chat_id)
            })
        }
    }

    impl FakeTelegramApi {
        fn media_result(&self, chat_id: i64) -> Result<TelegramMessageHandle, TelegramApiError> {
            if matches!(self.mode, FakeMode::MediaError) {
                return Err(TelegramApiError::Api {
                    code: 400,
                    description: "media failed".into(),
                });
            }
            Ok(TelegramMessageHandle {
                chat_id,
                message_id: 2,
            })
        }
    }

    fn assistant_message(content: Vec<ContentBlock>) -> AgentMessage {
        AgentMessage::assistant("assistant-1", content)
    }

    fn unique_test_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("noloong-delivery-{nanos}-{name}"))
    }
}
