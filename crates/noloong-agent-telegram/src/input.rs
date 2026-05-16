use crate::{
    access::{
        TelegramChatKind, TelegramReplyContext, TelegramReplyMediaKind, TelegramTextInput,
        telegram_text_mentions_username, telegram_text_without_username_mention,
        telegram_username_matches,
    },
    polling::{
        TelegramAudio, TelegramDocument, TelegramMessage, TelegramMessageEntity, TelegramPhotoSize,
        TelegramVideo, TelegramVoice,
    },
    text::{DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS, telegram_utf16_units},
};
use std::collections::BTreeMap;

const TELEGRAM_REPLY_TEXT_PREVIEW_LIMIT_UTF16_UNITS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramInboundUpdate {
    Message(TelegramInboundMessage),
    Command(TelegramCommand),
}

impl TelegramInboundUpdate {
    pub fn from_message(message: TelegramMessage, bot_username: Option<&str>) -> Option<Self> {
        TelegramCommand::from_message(&message, bot_username)
            .map(Self::Command)
            .or_else(|| {
                TelegramInboundMessage::from_message(message, bot_username).map(Self::Message)
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramInboundMessage {
    pub context: TelegramInboundContext,
    pub text: Option<String>,
    pub attachments: Vec<TelegramAttachment>,
}

impl TelegramInboundMessage {
    pub fn from_message(message: TelegramMessage, bot_username: Option<&str>) -> Option<Self> {
        let attachments = telegram_attachments(&message);
        let context = TelegramInboundContext::from_message(&message, bot_username);
        let text = message_text(message.text, message.caption);
        if text.is_none() && attachments.is_empty() {
            return None;
        }
        Some(Self {
            context,
            text,
            attachments,
        })
    }

    pub fn into_text_input(self) -> Option<TelegramTextInput> {
        if !self.attachments.is_empty() {
            return None;
        }
        Some(TelegramTextInput {
            chat_id: self.context.chat_id,
            thread_id: self.context.thread_id,
            chat_kind: self.context.chat_kind,
            user_id: self.context.user_id,
            message_id: self.context.message_id,
            text: self.text?,
            is_reply_to_bot: self.context.is_reply_to_bot,
            reply_to: self.context.reply_to,
        })
    }

    pub fn addresses_bot(&self, bot_username: Option<&str>) -> bool {
        self.context.is_reply_to_bot
            || self.text.as_deref().is_some_and(|text| {
                bot_username.is_some_and(|username| telegram_text_mentions_username(text, username))
            })
    }

    pub fn text_without_bot_mention(&self, bot_username: Option<&str>) -> Option<String> {
        let text = self.text.as_deref()?;
        let text = match bot_username {
            Some(username) => telegram_text_without_username_mention(text, username),
            None => text.trim().to_owned(),
        };
        (!text.trim().is_empty()).then_some(text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramCommand {
    pub context: TelegramInboundContext,
    pub name: String,
    pub bot_username: Option<String>,
    pub args: String,
    pub raw_text: String,
}

impl TelegramCommand {
    pub fn from_message(message: &TelegramMessage, bot_username: Option<&str>) -> Option<Self> {
        let text = message.text.as_ref()?;
        let command = parse_command_text(text, &message.entities, bot_username)?;
        Some(Self {
            context: TelegramInboundContext::from_message(message, bot_username),
            name: command.name,
            bot_username: command.bot_username,
            args: command.args,
            raw_text: text.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramInboundContext {
    pub chat_id: i64,
    pub thread_id: Option<i64>,
    pub chat_kind: TelegramChatKind,
    pub user_id: Option<u64>,
    pub message_id: i64,
    pub is_reply_to_bot: bool,
    pub reply_to: Option<TelegramReplyContext>,
}

impl TelegramInboundContext {
    fn from_message(message: &TelegramMessage, bot_username: Option<&str>) -> Self {
        Self {
            chat_id: message.chat.id,
            thread_id: message.message_thread_id,
            chat_kind: TelegramChatKind::parse(&message.chat.kind),
            user_id: message.from.as_ref().map(|user| user.id),
            message_id: message.message_id,
            is_reply_to_bot: message_is_reply_to_bot(message, bot_username),
            reply_to: message
                .reply_to_message
                .as_deref()
                .map(telegram_reply_context),
        }
    }

    pub fn from_text_input(input: &TelegramTextInput) -> Self {
        Self {
            chat_id: input.chat_id,
            thread_id: input.thread_id,
            chat_kind: input.chat_kind.clone(),
            user_id: input.user_id,
            message_id: input.message_id,
            is_reply_to_bot: input.is_reply_to_bot,
            reply_to: input.reply_to.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramAttachment {
    pub file: TelegramAttachmentFile,
    pub kind: TelegramAttachmentKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramAttachmentFile {
    pub file_id: String,
    pub file_unique_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<u64>,
}

impl TelegramAttachmentFile {
    pub(crate) fn new(
        file_id: &str,
        file_unique_id: &str,
        file_name: Option<&str>,
        mime_type: Option<&str>,
        file_size: Option<u64>,
    ) -> Self {
        Self {
            file_id: file_id.to_owned(),
            file_unique_id: file_unique_id.to_owned(),
            file_name: file_name.map(str::to_owned),
            mime_type: mime_type.map(str::to_owned),
            file_size,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramAttachmentKind {
    Photo {
        width: u32,
        height: u32,
    },
    Document,
    Audio {
        duration: u32,
    },
    Voice {
        duration: u32,
    },
    Video {
        width: u32,
        height: u32,
        duration: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramTextBatcherConfig {
    pub message_window_ms: u64,
    pub long_split_window_ms: u64,
}

impl Default for TelegramTextBatcherConfig {
    fn default() -> Self {
        Self {
            message_window_ms: 600,
            long_split_window_ms: 2_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramTextBatch {
    pub input: TelegramTextInput,
    pub ready_at_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TelegramTextBatcher {
    config: TelegramTextBatcherConfig,
    batches: BTreeMap<TelegramTextBatchKey, TelegramTextBatch>,
}

impl TelegramTextBatcher {
    pub fn new(config: TelegramTextBatcherConfig) -> Self {
        Self {
            config,
            batches: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, input: TelegramTextInput, now_ms: u64) {
        let key = TelegramTextBatchKey::from_input(&input);
        let delay_ms =
            if telegram_utf16_units(&input.text) >= DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS {
                self.config.long_split_window_ms
            } else {
                self.config.message_window_ms
            };
        let ready_at_ms = now_ms.saturating_add(delay_ms);
        self.batches
            .entry(key)
            .and_modify(|batch| {
                batch.input.message_id = input.message_id;
                batch.input.text = join_text(&batch.input.text, &input.text);
                batch.ready_at_ms = ready_at_ms;
            })
            .or_insert(TelegramTextBatch { input, ready_at_ms });
    }

    pub fn ready_batches(&mut self, now_ms: u64) -> Vec<TelegramTextBatch> {
        let ready_keys = self
            .batches
            .iter()
            .filter_map(|(key, batch)| (batch.ready_at_ms <= now_ms).then_some(*key))
            .collect::<Vec<_>>();
        ready_keys
            .into_iter()
            .filter_map(|key| self.batches.remove(&key))
            .collect()
    }

    pub fn flush_all(&mut self) -> Vec<TelegramTextBatch> {
        std::mem::take(&mut self.batches).into_values().collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TelegramTextBatchKey {
    chat_id: i64,
    thread_id: Option<i64>,
    user_id: Option<u64>,
    reply_to_message_id: Option<i64>,
}

impl TelegramTextBatchKey {
    fn from_input(input: &TelegramTextInput) -> Self {
        Self {
            chat_id: input.chat_id,
            thread_id: input.thread_id,
            user_id: input.user_id,
            reply_to_message_id: input.reply_to.as_ref().map(|reply| reply.message_id),
        }
    }
}

fn join_text(existing: &str, next: &str) -> String {
    if existing.is_empty() {
        return next.into();
    }
    if next.is_empty() {
        return existing.into();
    }
    format!("{existing}\n{next}")
}

fn message_text(text: Option<String>, caption: Option<String>) -> Option<String> {
    text.or(caption).and_then(trim_owned_text)
}

fn trim_owned_text(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == text.len() {
        return Some(text);
    }
    Some(trimmed.to_owned())
}

fn telegram_attachments(message: &TelegramMessage) -> Vec<TelegramAttachment> {
    let mut attachments = Vec::new();
    if let Some(photo) = best_photo_size(&message.photo) {
        attachments.push(TelegramAttachment::from_photo(photo));
    }
    if let Some(document) = &message.document {
        attachments.push(TelegramAttachment::from_document(document));
    }
    if let Some(audio) = &message.audio {
        attachments.push(TelegramAttachment::from_audio(audio));
    }
    if let Some(voice) = &message.voice {
        attachments.push(TelegramAttachment::from_voice(voice));
    }
    if let Some(video) = &message.video {
        attachments.push(TelegramAttachment::from_video(video));
    }
    attachments
}

fn telegram_reply_context(message: &TelegramMessage) -> TelegramReplyContext {
    TelegramReplyContext {
        message_id: message.message_id,
        chat_id: message.chat.id,
        thread_id: message.message_thread_id,
        user_id: message.from.as_ref().map(|user| user.id),
        username: message.from.as_ref().and_then(|user| user.username.clone()),
        text_preview: telegram_reply_text_preview(message),
        media_kinds: telegram_reply_media_kinds(message),
    }
}

fn telegram_reply_text_preview(message: &TelegramMessage) -> Option<String> {
    message_text(message.text.clone(), message.caption.clone())
        .map(|text| truncate_utf16_units(&text, TELEGRAM_REPLY_TEXT_PREVIEW_LIMIT_UTF16_UNITS))
}

fn telegram_reply_media_kinds(message: &TelegramMessage) -> Vec<TelegramReplyMediaKind> {
    let mut kinds = Vec::new();
    if !message.photo.is_empty() {
        kinds.push(TelegramReplyMediaKind::Photo);
    }
    if message.document.is_some() {
        kinds.push(TelegramReplyMediaKind::Document);
    }
    if message.audio.is_some() {
        kinds.push(TelegramReplyMediaKind::Audio);
    }
    if message.voice.is_some() {
        kinds.push(TelegramReplyMediaKind::Voice);
    }
    if message.video.is_some() {
        kinds.push(TelegramReplyMediaKind::Video);
    }
    kinds
}

fn truncate_utf16_units(text: &str, max_units: usize) -> String {
    let mut units = 0;
    let mut truncated = String::new();
    for ch in text.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max_units {
            break;
        }
        truncated.push(ch);
        units += ch_units;
    }
    truncated
}

fn best_photo_size(photo: &[TelegramPhotoSize]) -> Option<&TelegramPhotoSize> {
    photo.iter().max_by_key(|photo| {
        (
            photo.file_size.unwrap_or(0),
            u64::from(photo.width) * u64::from(photo.height),
        )
    })
}

impl TelegramAttachment {
    pub(crate) fn width(&self) -> Option<u32> {
        match self.kind {
            TelegramAttachmentKind::Photo { width, .. }
            | TelegramAttachmentKind::Video { width, .. } => Some(width),
            _ => None,
        }
    }

    pub(crate) fn height(&self) -> Option<u32> {
        match self.kind {
            TelegramAttachmentKind::Photo { height, .. }
            | TelegramAttachmentKind::Video { height, .. } => Some(height),
            _ => None,
        }
    }

    pub(crate) fn duration(&self) -> Option<u32> {
        match self.kind {
            TelegramAttachmentKind::Audio { duration }
            | TelegramAttachmentKind::Voice { duration }
            | TelegramAttachmentKind::Video { duration, .. } => Some(duration),
            _ => None,
        }
    }

    fn from_photo(photo: &TelegramPhotoSize) -> Self {
        Self {
            file: TelegramAttachmentFile::new(
                &photo.file_id,
                &photo.file_unique_id,
                None,
                None,
                photo.file_size,
            ),
            kind: TelegramAttachmentKind::Photo {
                width: photo.width,
                height: photo.height,
            },
        }
    }

    fn from_document(document: &TelegramDocument) -> Self {
        Self {
            file: TelegramAttachmentFile::new(
                &document.file_id,
                &document.file_unique_id,
                document.file_name.as_deref(),
                document.mime_type.as_deref(),
                document.file_size,
            ),
            kind: TelegramAttachmentKind::Document,
        }
    }

    fn from_audio(audio: &TelegramAudio) -> Self {
        Self {
            file: TelegramAttachmentFile::new(
                &audio.file_id,
                &audio.file_unique_id,
                audio.file_name.as_deref(),
                audio.mime_type.as_deref(),
                audio.file_size,
            ),
            kind: TelegramAttachmentKind::Audio {
                duration: audio.duration,
            },
        }
    }

    fn from_voice(voice: &TelegramVoice) -> Self {
        Self {
            file: TelegramAttachmentFile::new(
                &voice.file_id,
                &voice.file_unique_id,
                None,
                voice.mime_type.as_deref(),
                voice.file_size,
            ),
            kind: TelegramAttachmentKind::Voice {
                duration: voice.duration,
            },
        }
    }

    fn from_video(video: &TelegramVideo) -> Self {
        Self {
            file: TelegramAttachmentFile::new(
                &video.file_id,
                &video.file_unique_id,
                video.file_name.as_deref(),
                video.mime_type.as_deref(),
                video.file_size,
            ),
            kind: TelegramAttachmentKind::Video {
                width: video.width,
                height: video.height,
                duration: video.duration,
            },
        }
    }
}

struct ParsedCommand {
    name: String,
    bot_username: Option<String>,
    args: String,
}

fn parse_command_text(
    text: &str,
    entities: &[TelegramMessageEntity],
    bot_username: Option<&str>,
) -> Option<ParsedCommand> {
    if !entities
        .iter()
        .any(TelegramMessageEntity::is_bot_command_at_start)
        && !text.trim_start().starts_with('/')
    {
        return None;
    }
    let trimmed = text.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    let (token, args) = trimmed
        .split_once(char::is_whitespace)
        .map_or((trimmed, ""), |(token, args)| (token, args.trim()));
    let (name, command_bot_username) = parse_command_token(token)?;
    if let Some(command_bot_username) = &command_bot_username
        && !same_telegram_username(command_bot_username, bot_username)
    {
        return None;
    }
    Some(ParsedCommand {
        name,
        bot_username: command_bot_username,
        args: args.to_owned(),
    })
}

fn parse_command_token(token: &str) -> Option<(String, Option<String>)> {
    let command = token.strip_prefix('/')?;
    let (name, bot_username) = command
        .split_once('@')
        .map_or((command, None), |(name, username)| (name, Some(username)));
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some((name.to_ascii_lowercase(), bot_username.map(str::to_owned)))
}

fn message_is_reply_to_bot(message: &TelegramMessage, bot_username: Option<&str>) -> bool {
    message
        .reply_to_message
        .as_ref()
        .and_then(|reply| reply.from.as_ref())
        .and_then(|user| user.username.as_deref())
        .is_some_and(|username| same_telegram_username(username, bot_username))
}

fn same_telegram_username(username: &str, expected: Option<&str>) -> bool {
    telegram_username_matches(username, expected)
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramAttachmentKind, TelegramInboundUpdate, TelegramTextBatcher,
        TelegramTextBatcherConfig,
    };
    use crate::access::{
        TelegramChatKind, TelegramReplyContext, TelegramReplyMediaKind, TelegramTextInput,
    };
    use crate::polling::{
        TelegramChat, TelegramDocument, TelegramMessage, TelegramMessageEntity,
        TelegramMessageEntityKind, TelegramPhotoSize, TelegramUser,
    };

    #[test]
    fn text_batching_combines_continuous_messages() {
        let mut batcher = TelegramTextBatcher::new(TelegramTextBatcherConfig::default());
        batcher.push(input("hello", 1), 0);
        batcher.push(input("world", 2), 200);

        assert!(batcher.ready_batches(799).is_empty());
        let ready = batcher.ready_batches(800);

        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].input.text, "hello\nworld");
        assert_eq!(ready[0].input.message_id, 2);
    }

    #[test]
    fn split_threshold_batching_waits_longer() {
        let mut batcher = TelegramTextBatcher::new(TelegramTextBatcherConfig::default());
        batcher.push(input(&"x".repeat(3_900), 1), 0);

        assert!(batcher.ready_batches(1_999).is_empty());
        assert_eq!(batcher.ready_batches(2_000).len(), 1);
    }

    #[test]
    fn text_batching_separates_different_reply_targets() {
        let mut batcher = TelegramTextBatcher::new(TelegramTextBatcherConfig::default());
        let mut first = input("first", 1);
        first.reply_to = Some(reply_context(10));
        let mut second = input("second", 2);
        second.reply_to = Some(reply_context(11));
        let mut third = input("third", 3);
        third.reply_to = Some(reply_context(10));

        batcher.push(first, 0);
        batcher.push(second, 100);
        batcher.push(third, 200);
        let ready = batcher.ready_batches(800);

        assert_eq!(ready.len(), 2);
        assert!(ready.iter().any(|batch| batch.input.text == "first\nthird"));
        assert!(ready.iter().any(|batch| batch.input.text == "second"));
    }

    #[test]
    fn inbound_message_uses_caption_and_best_photo_attachment() {
        let update = TelegramInboundUpdate::from_message(
            TelegramMessage {
                message_id: 7,
                message_thread_id: Some(3),
                chat: TelegramChat {
                    id: -100,
                    kind: "supergroup".into(),
                },
                from: None,
                text: None,
                caption: Some("look".into()),
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: vec![
                    TelegramPhotoSize {
                        file_id: "small".into(),
                        file_unique_id: "small-unique".into(),
                        width: 90,
                        height: 90,
                        file_size: Some(100),
                    },
                    TelegramPhotoSize {
                        file_id: "large".into(),
                        file_unique_id: "large-unique".into(),
                        width: 1280,
                        height: 720,
                        file_size: Some(900),
                    },
                ],
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            },
            Some("noloong_bot"),
        )
        .unwrap();

        let TelegramInboundUpdate::Message(message) = update else {
            panic!("expected inbound message");
        };
        assert_eq!(message.text.as_deref(), Some("look"));
        assert_eq!(message.context.thread_id, Some(3));
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(
            message.attachments[0].kind,
            TelegramAttachmentKind::Photo {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(message.attachments[0].file.file_id, "large");
        assert!(message.into_text_input().is_none());
    }

    #[test]
    fn inbound_message_extracts_reply_context() {
        let update = TelegramInboundUpdate::from_message(
            TelegramMessage {
                message_id: 9,
                message_thread_id: Some(3),
                chat: TelegramChat {
                    id: -100,
                    kind: "supergroup".into(),
                },
                from: None,
                text: Some("what about this?".into()),
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: Some(Box::new(TelegramMessage {
                    message_id: 7,
                    message_thread_id: Some(3),
                    chat: TelegramChat {
                        id: -100,
                        kind: "supergroup".into(),
                    },
                    from: Some(TelegramUser {
                        id: 55,
                        username: Some("alice".into()),
                    }),
                    text: None,
                    caption: Some("see attached".into()),
                    entities: Vec::new(),
                    caption_entities: Vec::new(),
                    photo: vec![TelegramPhotoSize {
                        file_id: "photo".into(),
                        file_unique_id: "photo-unique".into(),
                        width: 640,
                        height: 480,
                        file_size: Some(100),
                    }],
                    document: Some(TelegramDocument {
                        file_id: "doc".into(),
                        file_unique_id: "doc-unique".into(),
                        file_name: Some("report.pdf".into()),
                        mime_type: Some("application/pdf".into()),
                        file_size: Some(200),
                    }),
                    audio: None,
                    voice: None,
                    video: None,
                    reply_to_message: None,
                })),
            },
            Some("noloong_bot"),
        )
        .unwrap();

        let TelegramInboundUpdate::Message(message) = update else {
            panic!("expected inbound message");
        };
        let reply = message.context.reply_to.as_ref().unwrap();
        assert_eq!(reply.message_id, 7);
        assert_eq!(reply.chat_id, -100);
        assert_eq!(reply.thread_id, Some(3));
        assert_eq!(reply.user_id, Some(55));
        assert_eq!(reply.username.as_deref(), Some("alice"));
        assert_eq!(reply.text_preview.as_deref(), Some("see attached"));
        assert_eq!(
            reply.media_kinds,
            [
                TelegramReplyMediaKind::Photo,
                TelegramReplyMediaKind::Document
            ]
        );
        assert_eq!(
            message
                .into_text_input()
                .unwrap()
                .reply_to
                .unwrap()
                .message_id,
            7
        );
    }

    #[test]
    fn command_detection_accepts_bot_suffix_and_keeps_args() {
        let update = TelegramInboundUpdate::from_message(
            TelegramMessage {
                message_id: 7,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: Some("/status@Noloong_Bot verbose".into()),
                caption: None,
                entities: vec![TelegramMessageEntity {
                    kind: TelegramMessageEntityKind::BotCommand,
                    offset: 0,
                    length: 19,
                }],
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            },
            Some("noloong_bot"),
        )
        .unwrap();

        let TelegramInboundUpdate::Command(command) = update else {
            panic!("expected command");
        };
        assert_eq!(command.name, "status");
        assert_eq!(command.bot_username.as_deref(), Some("Noloong_Bot"));
        assert_eq!(command.args, "verbose");
    }

    #[test]
    fn command_detection_accepts_plain_command() {
        let update = TelegramInboundUpdate::from_message(
            TelegramMessage {
                message_id: 7,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: Some("/help".into()),
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            },
            Some("noloong_bot"),
        )
        .unwrap();

        let TelegramInboundUpdate::Command(command) = update else {
            panic!("expected command");
        };
        assert_eq!(command.name, "help");
        assert_eq!(command.bot_username, None);
        assert_eq!(command.args, "");
    }

    #[test]
    fn command_detection_ignores_other_bot_suffix() {
        let update = TelegramInboundUpdate::from_message(
            TelegramMessage {
                message_id: 7,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: Some("/status@other_bot".into()),
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            },
            Some("noloong_bot"),
        )
        .unwrap();

        assert!(matches!(update, TelegramInboundUpdate::Message(_)));
    }

    fn input(text: &str, message_id: i64) -> TelegramTextInput {
        TelegramTextInput {
            chat_id: 42,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(7),
            message_id,
            text: text.into(),
            is_reply_to_bot: false,
            reply_to: None,
        }
    }

    fn reply_context(message_id: i64) -> TelegramReplyContext {
        TelegramReplyContext {
            message_id,
            chat_id: 42,
            thread_id: None,
            user_id: Some(7),
            username: Some("alice".into()),
            text_preview: Some(format!("reply {message_id}")),
            media_kinds: Vec::new(),
        }
    }
}
