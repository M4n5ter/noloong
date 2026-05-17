use noloong_agent::{ClientStateError, ClientStateKey, ClientStateStore, SqliteClientStateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
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

#[derive(Clone)]
pub struct SqliteWeixinStateStore {
    state: Arc<dyn ClientStateStore>,
    account_fingerprint: String,
}

impl std::fmt::Debug for SqliteWeixinStateStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteWeixinStateStore")
            .field("account_fingerprint", &self.account_fingerprint)
            .finish_non_exhaustive()
    }
}

impl SqliteWeixinStateStore {
    pub fn new(
        database_url: impl Into<String>,
        account_fingerprint: impl Into<String>,
    ) -> Result<Self, WeixinStateError> {
        let state = SqliteClientStateStore::new(database_url.into())?;
        Ok(Self::from_client_state(
            Arc::new(state),
            account_fingerprint,
        ))
    }

    pub fn from_client_state(
        state: Arc<dyn ClientStateStore>,
        account_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            state,
            account_fingerprint: account_fingerprint.into(),
        }
    }

    pub fn account_fingerprint(&self) -> &str {
        &self.account_fingerprint
    }

    fn key(&self, scope: &str, key: impl Into<String>) -> Result<ClientStateKey, WeixinStateError> {
        ClientStateKey::new("weixin", &self.account_fingerprint, scope, key.into())
            .map_err(WeixinStateError::client_state)
    }

    fn active_session_key(peer_id: &str, chat_kind: WeixinChatKind) -> String {
        format!("{}:{peer_id}", chat_kind.as_str())
    }
}

impl WeixinStateStore for SqliteWeixinStateStore {
    fn load_sync_buf<'a>(&'a self) -> WeixinStateFuture<'a, String> {
        Box::pin(async move {
            let key = self.key("sync", "buf")?;
            match self.state.get(&key).await? {
                Some(Value::String(sync_buf)) => Ok(sync_buf),
                Some(value) => Err(WeixinStateError::Decode(format!(
                    "weixin sync buffer is not a string: {value}"
                ))),
                None => Ok(String::new()),
            }
        })
    }

    fn save_sync_buf<'a>(&'a self, sync_buf: &'a str) -> WeixinStateFuture<'a, ()> {
        let sync_buf = sync_buf.to_owned();
        Box::pin(async move {
            let key = self.key("sync", "buf")?;
            self.state.set(&key, &Value::String(sync_buf)).await?;
            Ok(())
        })
    }

    fn context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, Option<String>> {
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            let key = self.key("contextToken", peer_id)?;
            match self.state.get(&key).await? {
                Some(Value::String(context_token)) => Ok(Some(context_token)),
                Some(value) => Err(WeixinStateError::Decode(format!(
                    "weixin context token is not a string: {value}"
                ))),
                None => Ok(None),
            }
        })
    }

    fn save_context_token<'a>(
        &'a self,
        peer_id: &'a str,
        context_token: &'a str,
    ) -> WeixinStateFuture<'a, ()> {
        let peer_id = peer_id.to_owned();
        let context_token = context_token.to_owned();
        Box::pin(async move {
            let key = self.key("contextToken", peer_id)?;
            self.state.set(&key, &Value::String(context_token)).await?;
            Ok(())
        })
    }

    fn delete_context_token<'a>(&'a self, peer_id: &'a str) -> WeixinStateFuture<'a, ()> {
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            let key = self.key("contextToken", peer_id)?;
            self.state.delete(&key).await?;
            Ok(())
        })
    }

    fn active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, Option<String>> {
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            let key = self.key(
                "activeSession",
                SqliteWeixinStateStore::active_session_key(&peer_id, chat_kind),
            )?;
            match self.state.get(&key).await? {
                Some(Value::String(session_id)) => Ok(Some(session_id)),
                Some(value) => Err(WeixinStateError::Decode(format!(
                    "weixin active session id is not a string: {value}"
                ))),
                None => Ok(None),
            }
        })
    }

    fn save_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
        session_id: &'a str,
    ) -> WeixinStateFuture<'a, ()> {
        let peer_id = peer_id.to_owned();
        let session_id = session_id.to_owned();
        Box::pin(async move {
            let key = self.key(
                "activeSession",
                SqliteWeixinStateStore::active_session_key(&peer_id, chat_kind),
            )?;
            self.state.set(&key, &Value::String(session_id)).await?;
            Ok(())
        })
    }

    fn delete_active_session_id<'a>(
        &'a self,
        peer_id: &'a str,
        chat_kind: WeixinChatKind,
    ) -> WeixinStateFuture<'a, ()> {
        let peer_id = peer_id.to_owned();
        Box::pin(async move {
            let key = self.key(
                "activeSession",
                SqliteWeixinStateStore::active_session_key(&peer_id, chat_kind),
            )?;
            self.state.delete(&key).await?;
            Ok(())
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
    #[error("Weixin client state failed: {0}")]
    ClientState(String),
    #[error("Weixin state sqlite failed: {0}")]
    Sqlite(String),
    #[error("Weixin state task failed: {0}")]
    Join(String),
}

impl WeixinStateError {
    fn client_state(error: impl std::fmt::Display) -> Self {
        Self::ClientState(error.to_string())
    }
}

impl From<ClientStateError> for WeixinStateError {
    fn from(error: ClientStateError) -> Self {
        Self::client_state(error)
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
        let store =
            SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account").unwrap();

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
