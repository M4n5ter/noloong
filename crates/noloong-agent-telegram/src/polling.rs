use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    future::Future,
    io::ErrorKind,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::config::TelegramStartupUpdatePolicy;
use crate::telegram_api::{TelegramApi, TelegramApiError};
use rusqlite::OptionalExtension;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramUpdate {
    pub update_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<TelegramMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramMessage {
    pub message_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
    pub chat: TelegramChat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<TelegramUser>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<TelegramMessageEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caption_entities: Vec<TelegramMessageEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photo: Vec<TelegramPhotoSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<TelegramDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<TelegramAudio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<TelegramVoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<TelegramVideo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_message: Option<Box<TelegramMessage>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramChat {
    pub id: i64,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramUser {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramMessageEntity {
    #[serde(rename = "type")]
    pub kind: TelegramMessageEntityKind,
    pub offset: usize,
    pub length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramMessageEntityKind {
    BotCommand,
    Unknown(String),
}

impl TelegramMessageEntityKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::BotCommand => "bot_command",
            Self::Unknown(value) => value,
        }
    }
}

impl Serialize for TelegramMessageEntityKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TelegramMessageEntityKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "bot_command" => Self::BotCommand,
            _ => Self::Unknown(value),
        })
    }
}

impl TelegramMessageEntity {
    pub fn is_bot_command_at_start(&self) -> bool {
        self.kind == TelegramMessageEntityKind::BotCommand && self.offset == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramPhotoSize {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramDocument {
    pub file_id: String,
    pub file_unique_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramAudio {
    pub file_id: String,
    pub file_unique_id: String,
    pub duration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramVoice {
    pub file_id: String,
    pub file_unique_id: String,
    pub duration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramVideo {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: u32,
    pub height: u32,
    pub duration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<TelegramMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramApiResponse<T> {
    pub ok: bool,
    pub result: T,
}

pub type TelegramUpdateHandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), TelegramPollingError>> + Send + 'a>>;

pub trait TelegramUpdateHandler: Send + Sync {
    fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a>;
}

pub type TelegramOffsetStoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TelegramOffsetStoreError>> + Send + 'a>>;

pub trait TelegramOffsetStore: Send + Sync {
    fn load<'a>(&'a self) -> TelegramOffsetStoreFuture<'a, Option<i64>>;

    fn save<'a>(&'a self, offset: i64) -> TelegramOffsetStoreFuture<'a, ()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTelegramOffsetStore {
    path: PathBuf,
}

impl FileTelegramOffsetStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl TelegramOffsetStore for FileTelegramOffsetStore {
    fn load<'a>(&'a self) -> TelegramOffsetStoreFuture<'a, Option<i64>> {
        Box::pin(async move {
            let text = match tokio::fs::read_to_string(&self.path).await {
                Ok(text) => text,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(TelegramOffsetStoreError::read(&self.path, error)),
            };
            let checkpoint = serde_json::from_str::<TelegramOffsetCheckpoint>(&text)
                .map_err(|error| TelegramOffsetStoreError::Decode(error.to_string()))?;
            Ok(Some(checkpoint.offset))
        })
    }

    fn save<'a>(&'a self, offset: i64) -> TelegramOffsetStoreFuture<'a, ()> {
        Box::pin(async move {
            if let Some(parent) = self.path.parent()
                && !parent.as_os_str().is_empty()
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| TelegramOffsetStoreError::write(parent, error))?;
            }
            let text = serde_json::to_string(&TelegramOffsetCheckpoint { offset })
                .map_err(|error| TelegramOffsetStoreError::Decode(error.to_string()))?;
            tokio::fs::write(&self.path, text)
                .await
                .map_err(|error| TelegramOffsetStoreError::write(&self.path, error))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteTelegramOffsetStore {
    database_url: String,
    bot_fingerprint: String,
}

impl SqliteTelegramOffsetStore {
    pub fn new(database_url: impl Into<String>, bot_fingerprint: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            bot_fingerprint: bot_fingerprint.into(),
        }
    }
}

impl TelegramOffsetStore for SqliteTelegramOffsetStore {
    fn load<'a>(&'a self) -> TelegramOffsetStoreFuture<'a, Option<i64>> {
        let database_url = self.database_url.clone();
        let bot_fingerprint = self.bot_fingerprint.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || load_sqlite_offset(&database_url, &bot_fingerprint))
                .await
                .map_err(TelegramOffsetStoreError::sqlite)?
        })
    }

    fn save<'a>(&'a self, offset: i64) -> TelegramOffsetStoreFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let bot_fingerprint = self.bot_fingerprint.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                save_sqlite_offset(&database_url, &bot_fingerprint, offset)
            })
            .await
            .map_err(TelegramOffsetStoreError::sqlite)?
        })
    }
}

fn load_sqlite_offset(
    database_url: &str,
    bot_fingerprint: &str,
) -> Result<Option<i64>, TelegramOffsetStoreError> {
    let connection = open_sqlite_offset_connection(database_url)?;
    ensure_sqlite_offset_schema(&connection)?;
    connection
        .query_row(
            "SELECT offset FROM telegram_offsets WHERE bot_fingerprint = ?1",
            [bot_fingerprint],
            |row| row.get(0),
        )
        .optional()
        .map_err(TelegramOffsetStoreError::sqlite)
}

fn save_sqlite_offset(
    database_url: &str,
    bot_fingerprint: &str,
    offset: i64,
) -> Result<(), TelegramOffsetStoreError> {
    let connection = open_sqlite_offset_connection(database_url)?;
    ensure_sqlite_offset_schema(&connection)?;
    connection
        .execute(
            "INSERT INTO telegram_offsets (bot_fingerprint, offset, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(bot_fingerprint) DO UPDATE SET
                 offset = excluded.offset,
                 updated_at_ms = excluded.updated_at_ms",
            rusqlite::params![bot_fingerprint, offset, current_unix_ms()],
        )
        .map_err(TelegramOffsetStoreError::sqlite)?;
    Ok(())
}

fn open_sqlite_offset_connection(
    database_url: &str,
) -> Result<rusqlite::Connection, TelegramOffsetStoreError> {
    match sqlite_offset_database_path(database_url)? {
        Some(path) => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|error| TelegramOffsetStoreError::write(parent, error))?;
            }
            rusqlite::Connection::open(path).map_err(TelegramOffsetStoreError::sqlite)
        }
        None => rusqlite::Connection::open_in_memory().map_err(TelegramOffsetStoreError::sqlite),
    }
}

fn ensure_sqlite_offset_schema(
    connection: &rusqlite::Connection,
) -> Result<(), TelegramOffsetStoreError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS telegram_offsets (
                bot_fingerprint TEXT PRIMARY KEY NOT NULL,
                offset INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
            [],
        )
        .map_err(TelegramOffsetStoreError::sqlite)?;
    Ok(())
}

fn sqlite_offset_database_path(
    database_url: &str,
) -> Result<Option<PathBuf>, TelegramOffsetStoreError> {
    match database_url {
        "" => Err(TelegramOffsetStoreError::Sqlite(
            "sqlite database url is empty".into(),
        )),
        "sqlite::memory:" | "sqlite://memory" | ":memory:" => Ok(None),
        url if url.starts_with("sqlite://") => {
            sqlite_offset_path_from_suffix(url.strip_prefix("sqlite://").unwrap_or_default())
        }
        url if url.starts_with("sqlite:") => {
            sqlite_offset_path_from_suffix(url.strip_prefix("sqlite:").unwrap_or_default())
        }
        url if url.contains("://") => Err(TelegramOffsetStoreError::Sqlite(format!(
            "telegram offset database URL must be sqlite, got: {url}"
        ))),
        path => Ok(Some(PathBuf::from(path))),
    }
}

fn sqlite_offset_path_from_suffix(path: &str) -> Result<Option<PathBuf>, TelegramOffsetStoreError> {
    if path.is_empty() {
        return Err(TelegramOffsetStoreError::Sqlite(
            "sqlite database path is empty".into(),
        ));
    }
    if path == ":memory:" || path == "memory" {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(path)))
}

fn current_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TelegramOffsetCheckpoint {
    offset: i64,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum TelegramOffsetStoreError {
    #[error("telegram offset checkpoint read failed: {0}")]
    Read(String),
    #[error("telegram offset checkpoint write failed: {0}")]
    Write(String),
    #[error("telegram offset checkpoint decode failed: {0}")]
    Decode(String),
    #[error("telegram offset sqlite failed: {0}")]
    Sqlite(String),
}

impl TelegramOffsetStoreError {
    fn read(path: &std::path::Path, error: std::io::Error) -> Self {
        Self::Read(format!("{}: {error}", path.display()))
    }

    fn write(path: &std::path::Path, error: std::io::Error) -> Self {
        Self::Write(format!("{}: {error}", path.display()))
    }

    fn sqlite(error: impl std::fmt::Display) -> Self {
        Self::Sqlite(error.to_string())
    }
}

#[derive(Clone)]
pub struct TelegramPoller {
    api: Arc<dyn TelegramApi>,
    handler: Arc<dyn TelegramUpdateHandler>,
    offset_store: Option<Arc<dyn TelegramOffsetStore>>,
    startup_update_policy: TelegramStartupUpdatePolicy,
    offset: Option<i64>,
    conflict_retries: u8,
    network_retries: u8,
    timeout_seconds: u64,
}

impl TelegramPoller {
    pub fn new(api: Arc<dyn TelegramApi>, handler: Arc<dyn TelegramUpdateHandler>) -> Self {
        Self {
            api,
            handler,
            offset_store: None,
            startup_update_policy: TelegramStartupUpdatePolicy::default(),
            offset: None,
            conflict_retries: 0,
            network_retries: 0,
            timeout_seconds: 50,
        }
    }

    pub fn with_offset_store(mut self, store: Arc<dyn TelegramOffsetStore>) -> Self {
        self.offset_store = Some(store);
        self
    }

    pub fn with_startup_update_policy(mut self, policy: TelegramStartupUpdatePolicy) -> Self {
        self.startup_update_policy = policy;
        self
    }

    pub fn offset(&self) -> Option<i64> {
        self.offset
    }

    pub async fn initialize(&mut self) -> Result<(), TelegramPollingError> {
        if self.offset.is_some() {
            return Ok(());
        }
        if let Some(store) = &self.offset_store
            && let Some(offset) = store.load().await?
        {
            self.offset = Some(offset);
            return Ok(());
        }
        if self.startup_update_policy == TelegramStartupUpdatePolicy::SkipPendingWithoutCheckpoint {
            self.skip_pending_updates().await?;
        }
        Ok(())
    }

    pub async fn poll_once(&mut self) -> Result<TelegramPollOutcome, TelegramPollingError> {
        match self
            .api
            .get_updates(self.offset, self.timeout_seconds)
            .await
        {
            Ok(updates) => {
                self.conflict_retries = 0;
                self.network_retries = 0;
                let mut latest_offset = None;
                for update in updates {
                    let next_offset = update.update_id + 1;
                    if is_supported_update(&update)
                        && let Err(error) = self.handler.handle_update(update).await
                    {
                        log::warn!("telegram update handler failed: {error}");
                    }
                    latest_offset = Some(next_offset);
                    self.offset = Some(next_offset);
                }
                if let Some(offset) = latest_offset {
                    self.save_offset(offset).await?;
                }
                Ok(TelegramPollOutcome::Polled)
            }
            Err(error) if error.retry_after_seconds().is_some() => {
                Ok(TelegramPollOutcome::RetryAfter {
                    delay_seconds: error.retry_after_seconds().unwrap_or(1),
                    reason: error.to_string(),
                })
            }
            Err(error) if error.is_conflict() => {
                self.conflict_retries += 1;
                if self.conflict_retries > 3 {
                    Err(TelegramPollingError::ConflictLimit)
                } else {
                    Ok(TelegramPollOutcome::RetryAfter {
                        delay_seconds: 10,
                        reason: "telegram getUpdates conflict".into(),
                    })
                }
            }
            Err(TelegramApiError::Network(message)) => {
                self.network_retries += 1;
                if self.network_retries > 10 {
                    return Err(TelegramPollingError::NetworkLimit(message));
                }
                Ok(TelegramPollOutcome::RetryAfter {
                    delay_seconds: network_backoff_seconds(self.network_retries),
                    reason: message,
                })
            }
            Err(error) => Err(TelegramPollingError::Api(error)),
        }
    }

    async fn skip_pending_updates(&mut self) -> Result<(), TelegramPollingError> {
        let updates = self.api.get_updates(None, 0).await?;
        let Some(next_offset) = updates.iter().map(|update| update.update_id + 1).max() else {
            return Ok(());
        };
        self.save_offset(next_offset).await
    }

    async fn save_offset(&mut self, offset: i64) -> Result<(), TelegramPollingError> {
        if let Some(store) = &self.offset_store {
            store.save(offset).await?;
        }
        self.offset = Some(offset);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramPollOutcome {
    Polled,
    RetryAfter { delay_seconds: u64, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramPollingError {
    #[error("telegram api failed: {0}")]
    Api(#[from] TelegramApiError),
    #[error("telegram polling conflict did not clear after retries")]
    ConflictLimit,
    #[error("telegram network did not recover after retries: {0}")]
    NetworkLimit(String),
    #[error("telegram update handler failed: {0}")]
    Handler(String),
    #[error("telegram offset checkpoint failed: {0}")]
    Offset(#[from] TelegramOffsetStoreError),
}

fn is_supported_update(update: &TelegramUpdate) -> bool {
    update
        .message
        .as_ref()
        .is_some_and(TelegramMessage::has_user_input)
        || update.callback_query.is_some()
}

impl TelegramMessage {
    pub fn has_user_input(&self) -> bool {
        self.has_text_input() || self.has_attachment_input()
    }

    pub fn has_text_input(&self) -> bool {
        self.text
            .as_deref()
            .or(self.caption.as_deref())
            .is_some_and(|text| !text.trim().is_empty())
    }

    pub fn has_attachment_input(&self) -> bool {
        !self.photo.is_empty()
            || self.document.is_some()
            || self.audio.is_some()
            || self.voice.is_some()
            || self.video.is_some()
    }
}

fn network_backoff_seconds(retries: u8) -> u64 {
    let delay = 5_u64.saturating_mul(1_u64 << retries.saturating_sub(1));
    delay.min(60)
}

#[cfg(test)]
mod tests {
    use super::{
        SqliteTelegramOffsetStore, TelegramMessageEntityKind, TelegramOffsetStore,
        TelegramOffsetStoreFuture, TelegramPollOutcome, TelegramPoller, TelegramPollingError,
        TelegramUpdateHandler, TelegramUpdateHandlerFuture,
    };
    use crate::{
        config::TelegramStartupUpdatePolicy,
        polling::{TelegramChat, TelegramMessage, TelegramUpdate},
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramEditMessageTextRequest, TelegramMessageHandle,
            TelegramSendMessageRequest,
        },
    };
    use std::{
        collections::VecDeque,
        future::Future,
        path::PathBuf,
        pin::Pin,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[tokio::test]
    async fn polling_advances_offset() {
        let api = Arc::new(FakeApi::with_updates(vec![
            text_update(7, "hello"),
            unsupported_update(8),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(poller.offset(), Some(9));
        assert_eq!(handler.handled_ids(), vec![7]);
    }

    #[tokio::test]
    async fn polling_handles_media_only_updates() {
        let api = Arc::new(FakeApi::with_updates(vec![photo_update(7)]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(poller.offset(), Some(8));
        assert_eq!(handler.handled_ids(), vec![7]);
    }

    #[tokio::test]
    async fn polling_persists_offset_after_handled_updates() {
        let api = Arc::new(FakeApi::with_updates(vec![text_update(7, "hello")]));
        let handler = Arc::new(FakeHandler::default());
        let store = Arc::new(FakeOffsetStore::default());
        let mut poller = TelegramPoller::new(api, handler).with_offset_store(store.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(store.offset(), Some(8));
    }

    #[tokio::test]
    async fn polling_persists_offset_once_per_successful_batch() {
        let api = Arc::new(FakeApi::with_updates(vec![
            text_update(7, "hello"),
            unsupported_update(8),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let store = Arc::new(FakeOffsetStore::default());
        let mut poller = TelegramPoller::new(api, handler).with_offset_store(store.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(store.offset(), Some(9));
        assert_eq!(store.save_count(), 1);
    }

    #[tokio::test]
    async fn sqlite_offset_store_round_trips_by_bot_fingerprint() {
        let path = unique_sqlite_path("telegram-offset");
        let url = format!("sqlite:{}", path.display());
        let first = SqliteTelegramOffsetStore::new(&url, "bot-a");

        assert_eq!(first.load().await.unwrap(), None);
        first.save(42).await.unwrap();
        first.save(43).await.unwrap();

        let reloaded = SqliteTelegramOffsetStore::new(&url, "bot-a");
        let other_bot = SqliteTelegramOffsetStore::new(&url, "bot-b");
        assert_eq!(reloaded.load().await.unwrap(), Some(43));
        assert_eq!(other_bot.load().await.unwrap(), None);

        remove_sqlite_files(&path);
    }

    #[tokio::test]
    async fn polling_advances_offset_when_handler_fails() {
        let api = Arc::new(FakeApi::with_updates(vec![text_update(7, "hello")]));
        let handler = Arc::new(FailingHandler);
        let store = Arc::new(FakeOffsetStore::default());
        let mut poller = TelegramPoller::new(api, handler).with_offset_store(store.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(poller.offset(), Some(8));
        assert_eq!(store.offset(), Some(8));
    }

    #[tokio::test]
    async fn startup_skip_pending_uses_checkpoint_when_missing() {
        let api = Arc::new(FakeApi::with_updates(vec![
            text_update(7, "old"),
            text_update(8, "older"),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let store = Arc::new(FakeOffsetStore::default());
        let mut poller = TelegramPoller::new(api.clone(), handler)
            .with_offset_store(store.clone())
            .with_startup_update_policy(TelegramStartupUpdatePolicy::SkipPendingWithoutCheckpoint);

        poller.initialize().await.unwrap();

        assert_eq!(poller.offset(), Some(9));
        assert_eq!(store.offset(), Some(9));
        assert_eq!(api.requested_offsets(), vec![None]);
    }

    #[tokio::test]
    async fn startup_uses_existing_checkpoint_before_skip_policy() {
        let api = Arc::new(FakeApi::with_updates(vec![text_update(7, "old")]));
        let handler = Arc::new(FakeHandler::default());
        let store = Arc::new(FakeOffsetStore::with_offset(12));
        let mut poller = TelegramPoller::new(api.clone(), handler)
            .with_offset_store(store)
            .with_startup_update_policy(TelegramStartupUpdatePolicy::SkipPendingWithoutCheckpoint);

        poller.initialize().await.unwrap();

        assert_eq!(poller.offset(), Some(12));
        assert_eq!(api.requested_offsets(), Vec::<Option<i64>>::new());
    }

    #[test]
    fn telegram_update_deserializes_bot_api_snake_case() {
        let update = serde_json::from_value::<TelegramUpdate>(serde_json::json!({
            "update_id": 7,
            "message": {
                "message_id": 11,
                "message_thread_id": 3,
                "chat": {"id": -100, "type": "supergroup"},
                "from": {"id": 42, "username": "alice"},
                "text": "hello",
                "reply_to_message": {
                    "message_id": 10,
                    "chat": {"id": -100, "type": "supergroup"},
                    "from": {"id": 1, "username": "noloong_bot"},
                    "text": "previous"
                }
            }
        }))
        .unwrap();

        let message = update.message.unwrap();
        assert_eq!(update.update_id, 7);
        assert_eq!(message.message_id, 11);
        assert_eq!(message.message_thread_id, Some(3));
        assert_eq!(message.chat.kind, "supergroup");
        assert_eq!(
            message
                .reply_to_message
                .as_ref()
                .and_then(|reply| reply.from.as_ref())
                .and_then(|user| user.username.as_deref()),
            Some("noloong_bot")
        );
    }

    #[test]
    fn telegram_message_deserializes_rich_media_fields() {
        let update = serde_json::from_value::<TelegramUpdate>(serde_json::json!({
            "update_id": 7,
            "message": {
                "message_id": 11,
                "message_thread_id": 3,
                "chat": {"id": -100, "type": "supergroup"},
                "from": {"id": 42, "username": "alice"},
                "caption": "see attached",
                "caption_entities": [{"type": "bot_command", "offset": 0, "length": 4}],
                "photo": [{
                    "file_id": "photo-1",
                    "file_unique_id": "photo-u1",
                    "width": 800,
                    "height": 600,
                    "file_size": 123
                }],
                "document": {
                    "file_id": "doc-1",
                    "file_unique_id": "doc-u1",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 456
                },
                "audio": {
                    "file_id": "audio-1",
                    "file_unique_id": "audio-u1",
                    "duration": 12,
                    "file_name": "sound.mp3",
                    "mime_type": "audio/mpeg",
                    "file_size": 789
                },
                "voice": {
                    "file_id": "voice-1",
                    "file_unique_id": "voice-u1",
                    "duration": 5,
                    "mime_type": "audio/ogg",
                    "file_size": 111
                },
                "video": {
                    "file_id": "video-1",
                    "file_unique_id": "video-u1",
                    "width": 1280,
                    "height": 720,
                    "duration": 20,
                    "file_name": "clip.mp4",
                    "mime_type": "video/mp4",
                    "file_size": 222
                }
            }
        }))
        .unwrap();

        let message = update.message.unwrap();
        assert_eq!(message.caption.as_deref(), Some("see attached"));
        assert_eq!(
            message.caption_entities[0].kind,
            TelegramMessageEntityKind::BotCommand
        );
        assert_eq!(message.photo[0].file_id, "photo-1");
        assert_eq!(
            message.document.unwrap().file_name.as_deref(),
            Some("report.pdf")
        );
        assert_eq!(message.audio.unwrap().duration, 12);
        assert_eq!(
            message.voice.unwrap().mime_type.as_deref(),
            Some("audio/ogg")
        );
        assert_eq!(message.video.unwrap().width, 1280);
    }

    #[tokio::test]
    async fn polling_retries_network_errors() {
        let api = Arc::new(FakeApi::with_errors(vec![TelegramApiError::Network(
            "timeout".into(),
        )]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler);

        let outcome = poller.poll_once().await.unwrap();

        assert_eq!(
            outcome,
            TelegramPollOutcome::RetryAfter {
                delay_seconds: 5,
                reason: "timeout".into(),
            }
        );
    }

    #[tokio::test]
    async fn polling_uses_telegram_retry_after() {
        let api = Arc::new(FakeApi::with_errors(vec![rate_limited(7)]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler);

        let outcome = poller.poll_once().await.unwrap();

        assert_eq!(
            outcome,
            TelegramPollOutcome::RetryAfter {
                delay_seconds: 7,
                reason: "telegram api error 429: Too Many Requests".into(),
            }
        );
    }

    #[tokio::test]
    async fn polling_conflict_becomes_fatal_after_retries() {
        let api = Arc::new(FakeApi::with_errors(vec![
            conflict(),
            conflict(),
            conflict(),
            conflict(),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler);

        assert!(poller.poll_once().await.is_ok());
        assert!(poller.poll_once().await.is_ok());
        assert!(poller.poll_once().await.is_ok());
        assert!(matches!(
            poller.poll_once().await.unwrap_err(),
            TelegramPollingError::ConflictLimit
        ));
    }

    fn text_update(update_id: i64, text: &str) -> TelegramUpdate {
        TelegramUpdate {
            update_id,
            message: Some(TelegramMessage {
                message_id: update_id,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: Some(text.into()),
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            }),
            callback_query: None,
        }
    }

    fn unsupported_update(update_id: i64) -> TelegramUpdate {
        TelegramUpdate {
            update_id,
            message: None,
            callback_query: None,
        }
    }

    fn photo_update(update_id: i64) -> TelegramUpdate {
        TelegramUpdate {
            update_id,
            message: Some(TelegramMessage {
                message_id: update_id,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: None,
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: vec![super::TelegramPhotoSize {
                    file_id: "photo-1".into(),
                    file_unique_id: "photo-u1".into(),
                    width: 800,
                    height: 600,
                    file_size: Some(123),
                }],
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            }),
            callback_query: None,
        }
    }

    fn conflict() -> TelegramApiError {
        TelegramApiError::Api {
            code: 409,
            description: "terminated by other getUpdates request".into(),
            retry_after: None,
        }
    }

    fn rate_limited(retry_after: u64) -> TelegramApiError {
        TelegramApiError::Api {
            code: 429,
            description: "Too Many Requests".into(),
            retry_after: Some(retry_after),
        }
    }

    #[derive(Default)]
    struct FakeHandler {
        handled: Mutex<Vec<i64>>,
    }

    impl FakeHandler {
        fn handled_ids(&self) -> Vec<i64> {
            self.handled
                .lock()
                .expect("fake handled lock poisoned")
                .clone()
        }
    }

    impl TelegramUpdateHandler for FakeHandler {
        fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a> {
            Box::pin(async move {
                self.handled
                    .lock()
                    .expect("fake handled lock poisoned")
                    .push(update.update_id);
                Ok(())
            })
        }
    }

    struct FailingHandler;

    impl TelegramUpdateHandler for FailingHandler {
        fn handle_update<'a>(&'a self, _update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a> {
            Box::pin(async { Err(TelegramPollingError::Handler("boom".into())) })
        }
    }

    struct FakeApi {
        updates: Mutex<VecDeque<Vec<TelegramUpdate>>>,
        errors: Mutex<VecDeque<TelegramApiError>>,
        requested_offsets: Mutex<Vec<Option<i64>>>,
    }

    impl FakeApi {
        fn with_updates(updates: Vec<TelegramUpdate>) -> Self {
            Self {
                updates: Mutex::new(VecDeque::from([updates])),
                errors: Mutex::new(VecDeque::new()),
                requested_offsets: Mutex::new(Vec::new()),
            }
        }

        fn with_errors(errors: Vec<TelegramApiError>) -> Self {
            Self {
                updates: Mutex::new(VecDeque::new()),
                errors: Mutex::new(errors.into()),
                requested_offsets: Mutex::new(Vec::new()),
            }
        }

        fn requested_offsets(&self) -> Vec<Option<i64>> {
            self.requested_offsets
                .lock()
                .expect("fake requested offsets lock poisoned")
                .clone()
        }
    }

    impl TelegramApi for FakeApi {
        fn get_updates<'a>(
            &'a self,
            offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.requested_offsets
                    .lock()
                    .expect("fake requested offsets lock poisoned")
                    .push(offset);
                if let Some(error) = self
                    .errors
                    .lock()
                    .expect("fake errors lock poisoned")
                    .pop_front()
                {
                    return Err(error);
                }
                Ok(self
                    .updates
                    .lock()
                    .expect("fake updates lock poisoned")
                    .pop_front()
                    .unwrap_or_default())
            })
        }

        fn send_message<'a>(
            &'a self,
            _request: TelegramSendMessageRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(TelegramMessageHandle {
                    chat_id: 42,
                    message_id: 1,
                })
            })
        }

        fn edit_message_text<'a>(
            &'a self,
            _request: TelegramEditMessageTextRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(TelegramMessageHandle {
                    chat_id: 42,
                    message_id: 1,
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
    }

    fn unique_sqlite_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "noloong-{name}-{}-{nanos}.sqlite",
            std::process::id()
        ))
    }

    fn remove_sqlite_files(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    }

    #[derive(Default)]
    struct FakeOffsetStore {
        offset: Mutex<Option<i64>>,
        saves: Mutex<usize>,
    }

    impl FakeOffsetStore {
        fn with_offset(offset: i64) -> Self {
            Self {
                offset: Mutex::new(Some(offset)),
                saves: Mutex::new(0),
            }
        }

        fn offset(&self) -> Option<i64> {
            *self.offset.lock().expect("fake offset lock poisoned")
        }

        fn save_count(&self) -> usize {
            *self.saves.lock().expect("fake saves lock poisoned")
        }
    }

    impl TelegramOffsetStore for FakeOffsetStore {
        fn load<'a>(&'a self) -> TelegramOffsetStoreFuture<'a, Option<i64>> {
            Box::pin(async move { Ok(self.offset()) })
        }

        fn save<'a>(&'a self, offset: i64) -> TelegramOffsetStoreFuture<'a, ()> {
            Box::pin(async move {
                *self.offset.lock().expect("fake offset lock poisoned") = Some(offset);
                *self.saves.lock().expect("fake saves lock poisoned") += 1;
                Ok(())
            })
        }
    }
}
