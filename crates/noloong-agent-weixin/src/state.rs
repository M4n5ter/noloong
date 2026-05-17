use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

use crate::session::WeixinChatKind;

pub type WeixinStateFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WeixinStateError>> + Send + 'a>>;

pub trait WeixinStateStore: Send + Sync {
    fn load_sync_buf<'a>(&'a self) -> WeixinStateFuture<'a, String>;

    fn save_sync_buf<'a>(&'a self, sync_buf: &'a str) -> WeixinStateFuture<'a, ()>;

    fn context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, Option<String>>;

    fn save_context_token<'a>(
        &'a self,
        peer_id: &'a str,
        context_token: &'a str,
    ) -> WeixinStateFuture<'a, ()>;

    fn delete_context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, ()>;

    fn active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, Option<String>>;

    fn save_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
        session_id: &'a str,
    ) -> WeixinStateFuture<'a, ()>;

    fn delete_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, ()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteWeixinStateStore {
    database_url: String,
    account_fingerprint: String,
}

impl SqliteWeixinStateStore {
    pub fn new(database_url: impl Into<String>, account_fingerprint: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            account_fingerprint: account_fingerprint.into(),
        }
    }

    pub fn account_fingerprint(&self) -> &str {
        &self.account_fingerprint
    }
}

impl WeixinStateStore for SqliteWeixinStateStore {
    fn load_sync_buf<'a>(&'a self) -> WeixinStateFuture<'a, String> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .query_row(
                        "SELECT sync_buf FROM weixin_sync_state WHERE account_fingerprint = ?1",
                        [account_fingerprint],
                        |row| row.get(0),
                    )
                    .optional()
                    .map(|value| value.unwrap_or_default())
                    .map_err(WeixinStateError::sqlite)
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn save_sync_buf<'a>(&'a self, sync_buf: &'a str) -> WeixinStateFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let sync_buf = sync_buf.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .execute(
                        "INSERT INTO weixin_sync_state (account_fingerprint, sync_buf, updated_at_ms)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(account_fingerprint) DO UPDATE SET
                             sync_buf = excluded.sync_buf,
                             updated_at_ms = excluded.updated_at_ms",
                        rusqlite::params![account_fingerprint, sync_buf, current_unix_ms_i64()],
                    )
                    .map_err(WeixinStateError::sqlite)?;
                Ok(())
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, Option<String>> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .query_row(
                        "SELECT context_token FROM weixin_context_tokens
                         WHERE account_fingerprint = ?1 AND peer_id = ?2",
                        rusqlite::params![account_fingerprint, peer_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(WeixinStateError::sqlite)
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn save_context_token<'a>(
        &'a self,
        peer_id: &'a str,
        context_token: &'a str,
    ) -> WeixinStateFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        let context_token = context_token.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .execute(
                        "INSERT INTO weixin_context_tokens
                             (account_fingerprint, peer_id, context_token, updated_at_ms)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(account_fingerprint, peer_id) DO UPDATE SET
                             context_token = excluded.context_token,
                             updated_at_ms = excluded.updated_at_ms",
                        rusqlite::params![
                            account_fingerprint,
                            peer_id,
                            context_token,
                            current_unix_ms_i64()
                        ],
                    )
                    .map_err(WeixinStateError::sqlite)?;
                Ok(())
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn delete_context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .execute(
                        "DELETE FROM weixin_context_tokens
                         WHERE account_fingerprint = ?1 AND peer_id = ?2",
                        rusqlite::params![account_fingerprint, peer_id],
                    )
                    .map_err(WeixinStateError::sqlite)?;
                Ok(())
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, Option<String>> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .query_row(
                        "SELECT session_id FROM weixin_active_sessions
                         WHERE account_fingerprint = ?1 AND peer_id = ?2 AND chat_kind = ?3",
                        rusqlite::params![account_fingerprint, peer_id, chat_kind.as_str()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(WeixinStateError::sqlite)
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn save_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
        session_id: &'a str,
    ) -> WeixinStateFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        let session_id = session_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .execute(
                        "INSERT INTO weixin_active_sessions
                             (account_fingerprint, peer_id, chat_kind, session_id, updated_at_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(account_fingerprint, peer_id, chat_kind) DO UPDATE SET
                             session_id = excluded.session_id,
                             updated_at_ms = excluded.updated_at_ms",
                        rusqlite::params![
                            account_fingerprint,
                            peer_id,
                            chat_kind.as_str(),
                            session_id,
                            current_unix_ms_i64()
                        ],
                    )
                    .map_err(WeixinStateError::sqlite)?;
                Ok(())
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }

    fn delete_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, ()> {
        let database_url = self.database_url.clone();
        let account_fingerprint = self.account_fingerprint.clone();
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let connection = open_sqlite_connection(&database_url)?;
                ensure_schema(&connection)?;
                connection
                    .execute(
                        "DELETE FROM weixin_active_sessions
                         WHERE account_fingerprint = ?1 AND peer_id = ?2 AND chat_kind = ?3",
                        rusqlite::params![account_fingerprint, peer_id, chat_kind.as_str()],
                    )
                    .map_err(WeixinStateError::sqlite)?;
                Ok(())
            })
            .await
            .map_err(WeixinStateError::join)?
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinStoredAccount {
    pub account_id: String,
    pub token: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub saved_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinAccountStore {
    root: PathBuf,
}

impl WeixinAccountStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new(
            home.join(".agents")
                .join("noloong")
                .join("weixin")
                .join("accounts"),
        )
    }

    pub fn account_path(&self, account_id: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", sanitize_file_component(account_id)))
    }

    pub fn save(&self, account: &WeixinStoredAccount) -> Result<(), WeixinStateError> {
        std::fs::create_dir_all(&self.root)
            .map_err(|error| WeixinStateError::Io(format!("{}: {error}", self.root.display())))?;
        let path = self.account_path(&account.account_id);
        let text = serde_json::to_string_pretty(account)
            .map_err(|error| WeixinStateError::Decode(error.to_string()))?;
        std::fs::write(&path, text)
            .map_err(|error| WeixinStateError::Io(format!("{}: {error}", path.display())))?;
        set_owner_only_permissions(&path);
        Ok(())
    }

    pub fn load(&self, account_id: &str) -> Result<Option<WeixinStoredAccount>, WeixinStateError> {
        let path = self.account_path(account_id);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(WeixinStateError::Io(format!("{}: {error}", path.display())));
            }
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| WeixinStateError::Decode(error.to_string()))
    }
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) {}

fn open_sqlite_connection(database_url: &str) -> Result<rusqlite::Connection, WeixinStateError> {
    match sqlite_database_path(database_url)? {
        Some(path) => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).map_err(|error| {
                    WeixinStateError::Io(format!("{}: {error}", parent.display()))
                })?;
            }
            rusqlite::Connection::open(path).map_err(WeixinStateError::sqlite)
        }
        None => rusqlite::Connection::open_in_memory().map_err(WeixinStateError::sqlite),
    }
}

fn ensure_schema(connection: &rusqlite::Connection) -> Result<(), WeixinStateError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS weixin_sync_state (
                account_fingerprint TEXT PRIMARY KEY NOT NULL,
                sync_buf TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS weixin_context_tokens (
                account_fingerprint TEXT NOT NULL,
                peer_id TEXT NOT NULL,
                context_token TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                PRIMARY KEY(account_fingerprint, peer_id)
            );
            CREATE TABLE IF NOT EXISTS weixin_active_sessions (
                account_fingerprint TEXT NOT NULL,
                peer_id TEXT NOT NULL,
                chat_kind TEXT NOT NULL,
                session_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                PRIMARY KEY(account_fingerprint, peer_id, chat_kind)
            );",
        )
        .map_err(WeixinStateError::sqlite)?;
    Ok(())
}

fn sqlite_database_path(database_url: &str) -> Result<Option<PathBuf>, WeixinStateError> {
    match database_url {
        "" => Err(WeixinStateError::Sqlite(
            "sqlite database url is empty".into(),
        )),
        "sqlite::memory:" | "sqlite://memory" | ":memory:" => Ok(None),
        url if url.starts_with("sqlite://") => {
            sqlite_path_from_suffix(url.strip_prefix("sqlite://").unwrap_or_default())
        }
        url if url.starts_with("sqlite:") => {
            sqlite_path_from_suffix(url.strip_prefix("sqlite:").unwrap_or_default())
        }
        url if url.contains("://") => Err(WeixinStateError::Sqlite(format!(
            "weixin state database URL must be sqlite, got: {url}"
        ))),
        path => Ok(Some(PathBuf::from(path))),
    }
}

fn sqlite_path_from_suffix(path: &str) -> Result<Option<PathBuf>, WeixinStateError> {
    if path.is_empty() {
        return Err(WeixinStateError::Sqlite(
            "sqlite database path is empty".into(),
        ));
    }
    if path == ":memory:" || path == "memory" {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(path)))
}

pub fn account_fingerprint(account_id: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in account_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn current_unix_ms_i64() -> i64 {
    current_unix_ms().min(i64::MAX as u64) as i64
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "account".into()
    } else {
        sanitized
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum WeixinStateError {
    #[error("Weixin state I/O failed: {0}")]
    Io(String),
    #[error("Weixin state decode failed: {0}")]
    Decode(String),
    #[error("Weixin state sqlite failed: {0}")]
    Sqlite(String),
    #[error("Weixin state task failed: {0}")]
    Join(String),
}

impl WeixinStateError {
    fn sqlite(error: impl std::fmt::Display) -> Self {
        Self::Sqlite(error.to_string())
    }

    fn join(error: impl std::fmt::Display) -> Self {
        Self::Join(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{SqliteWeixinStateStore, WeixinStateStore};
    use crate::session::WeixinChatKind;

    #[tokio::test]
    async fn sqlite_state_round_trips_sync_and_context_token() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-state-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let store = SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account");

        assert_eq!(store.load_sync_buf().await.unwrap(), "");
        store.save_sync_buf("sync-1").await.unwrap();
        assert_eq!(store.load_sync_buf().await.unwrap(), "sync-1");

        assert_eq!(store.context_token("user").await.unwrap(), None);
        store.save_context_token("user", "ctx-1").await.unwrap();
        assert_eq!(
            store.context_token("user").await.unwrap().as_deref(),
            Some("ctx-1")
        );
        store.delete_context_token("user").await.unwrap();
        assert_eq!(store.context_token("user").await.unwrap(), None);

        assert_eq!(
            store
                .active_session_id("user", WeixinChatKind::Dm)
                .await
                .unwrap(),
            None
        );
        store
            .save_active_session_id("user", WeixinChatKind::Dm, "session-1")
            .await
            .unwrap();
        assert_eq!(
            store
                .active_session_id("user", WeixinChatKind::Dm)
                .await
                .unwrap()
                .as_deref(),
            Some("session-1")
        );
        store
            .delete_active_session_id("user", WeixinChatKind::Dm)
            .await
            .unwrap();
        assert_eq!(
            store
                .active_session_id("user", WeixinChatKind::Dm)
                .await
                .unwrap(),
            None
        );
        let _ = std::fs::remove_file(path);
    }
}
