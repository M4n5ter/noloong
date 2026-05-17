use serde_json::Value;
use std::{future::Future, pin::Pin};
use thiserror::Error;

pub type ClientStateFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ClientStateError>> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientStateKey {
    client: String,
    account: String,
    scope: String,
    key: String,
}

impl ClientStateKey {
    pub fn new(
        client: impl Into<String>,
        account: impl Into<String>,
        scope: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self, ClientStateError> {
        let key = Self {
            client: client.into(),
            account: account.into(),
            scope: scope.into(),
            key: key.into(),
        };
        key.validate()?;
        Ok(key)
    }

    fn validate(&self) -> Result<(), ClientStateError> {
        for (label, value) in [
            ("client", self.client.as_str()),
            ("account", self.account.as_str()),
            ("scope", self.scope.as_str()),
            ("key", self.key.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ClientStateError::InvalidKey(format!(
                    "client state {label} must not be empty"
                )));
            }
        }
        Ok(())
    }

    #[cfg(feature = "client-state-sqlite")]
    fn parts_owned(&self) -> (String, String, String, String) {
        (
            self.client.clone(),
            self.account.clone(),
            self.scope.clone(),
            self.key.clone(),
        )
    }
}

pub trait ClientStateStore: Send + Sync {
    fn get<'a>(&'a self, key: &'a ClientStateKey) -> ClientStateFuture<'a, Option<Value>>;

    fn set<'a>(&'a self, key: &'a ClientStateKey, value: &'a Value) -> ClientStateFuture<'a, ()>;

    fn delete<'a>(&'a self, key: &'a ClientStateKey) -> ClientStateFuture<'a, ()>;
}

#[cfg(feature = "client-state-sqlite")]
mod sqlite {
    use super::{ClientStateError, ClientStateFuture, ClientStateKey, ClientStateStore};
    use rusqlite::OptionalExtension;
    use serde_json::Value;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone)]
    pub struct SqliteClientStateStore {
        backend: SqliteClientStateBackend,
    }

    #[derive(Clone)]
    enum SqliteClientStateBackend {
        File(PathBuf),
        Memory(Arc<Mutex<rusqlite::Connection>>),
    }

    impl std::fmt::Debug for SqliteClientStateStore {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.backend {
                SqliteClientStateBackend::File(path) => formatter
                    .debug_struct("SqliteClientStateStore")
                    .field("path", path)
                    .finish(),
                SqliteClientStateBackend::Memory(_) => formatter
                    .debug_struct("SqliteClientStateStore")
                    .field("path", &":memory:")
                    .finish(),
            }
        }
    }

    impl SqliteClientStateStore {
        pub fn new(database_url: impl AsRef<str>) -> Result<Self, ClientStateError> {
            match sqlite_database_path(database_url.as_ref())? {
                Some(path) => {
                    if let Some(parent) = path
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                    {
                        std::fs::create_dir_all(parent).map_err(|error| {
                            ClientStateError::Io(format!("{}: {error}", parent.display()))
                        })?;
                    }
                    let connection =
                        rusqlite::Connection::open(&path).map_err(ClientStateError::sqlite)?;
                    ensure_schema(&connection)?;
                    Ok(Self {
                        backend: SqliteClientStateBackend::File(path),
                    })
                }
                None => {
                    let connection =
                        rusqlite::Connection::open_in_memory().map_err(ClientStateError::sqlite)?;
                    ensure_schema(&connection)?;
                    Ok(Self {
                        backend: SqliteClientStateBackend::Memory(Arc::new(Mutex::new(connection))),
                    })
                }
            }
        }

        fn with_connection<'a, T, F>(&'a self, action: F) -> ClientStateFuture<'a, T>
        where
            T: Send + 'static,
            F: FnOnce(&rusqlite::Connection) -> Result<T, ClientStateError> + Send + 'static,
        {
            let backend = self.backend.clone();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || match backend {
                    SqliteClientStateBackend::File(path) => {
                        let connection =
                            rusqlite::Connection::open(path).map_err(ClientStateError::sqlite)?;
                        action(&connection)
                    }
                    SqliteClientStateBackend::Memory(connection) => {
                        let connection = connection.lock().map_err(|_| {
                            ClientStateError::Sqlite("connection lock poisoned".into())
                        })?;
                        action(&connection)
                    }
                })
                .await
                .map_err(|error| ClientStateError::Sqlite(format!("sqlite task failed: {error}")))?
            })
        }
    }

    impl ClientStateStore for SqliteClientStateStore {
        fn get<'a>(&'a self, key: &'a ClientStateKey) -> ClientStateFuture<'a, Option<Value>> {
            let (client, account, scope, key) = key.parts_owned();
            self.with_connection(move |connection| {
                connection
                    .query_row(
                        "SELECT value_json FROM client_state_entries
                         WHERE client = ?1 AND account = ?2 AND scope = ?3 AND key = ?4",
                        rusqlite::params![client, account, scope, key],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(ClientStateError::sqlite)?
                    .map(|json| serde_json::from_str(&json).map_err(ClientStateError::json))
                    .transpose()
            })
        }

        fn set<'a>(
            &'a self,
            key: &'a ClientStateKey,
            value: &'a Value,
        ) -> ClientStateFuture<'a, ()> {
            let (client, account, scope, key) = key.parts_owned();
            let value_json = match serde_json::to_string(value) {
                Ok(value_json) => value_json,
                Err(error) => return Box::pin(async move { Err(ClientStateError::json(error)) }),
            };
            self.with_connection(move |connection| {
                connection
                    .execute(
                        "INSERT INTO client_state_entries
                             (client, account, scope, key, value_json, updated_at_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(client, account, scope, key) DO UPDATE SET
                             value_json = excluded.value_json,
                             updated_at_ms = excluded.updated_at_ms",
                        rusqlite::params![
                            client,
                            account,
                            scope,
                            key,
                            value_json,
                            current_unix_ms_i64()
                        ],
                    )
                    .map_err(ClientStateError::sqlite)?;
                Ok(())
            })
        }

        fn delete<'a>(&'a self, key: &'a ClientStateKey) -> ClientStateFuture<'a, ()> {
            let (client, account, scope, key) = key.parts_owned();
            self.with_connection(move |connection| {
                connection
                    .execute(
                        "DELETE FROM client_state_entries
                         WHERE client = ?1 AND account = ?2 AND scope = ?3 AND key = ?4",
                        rusqlite::params![client, account, scope, key],
                    )
                    .map_err(ClientStateError::sqlite)?;
                Ok(())
            })
        }
    }

    fn ensure_schema(connection: &rusqlite::Connection) -> Result<(), ClientStateError> {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS client_state_entries (
                    client TEXT NOT NULL,
                    account TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    key TEXT NOT NULL,
                    value_json TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    PRIMARY KEY(client, account, scope, key)
                );",
            )
            .map_err(ClientStateError::sqlite)?;
        Ok(())
    }

    fn sqlite_database_path(database_url: &str) -> Result<Option<PathBuf>, ClientStateError> {
        match database_url {
            "" => Err(ClientStateError::Sqlite(
                "sqlite database url is empty".into(),
            )),
            "sqlite::memory:" | "sqlite://memory" | ":memory:" => Ok(None),
            url if url.starts_with("sqlite://") => {
                sqlite_path_from_suffix(url.strip_prefix("sqlite://").unwrap_or_default())
            }
            url if url.starts_with("sqlite:") => {
                sqlite_path_from_suffix(url.strip_prefix("sqlite:").unwrap_or_default())
            }
            url if url.contains("://") => Err(ClientStateError::Sqlite(format!(
                "client state database URL must be sqlite, got: {url}"
            ))),
            path => Ok(Some(PathBuf::from(path))),
        }
    }

    fn sqlite_path_from_suffix(path: &str) -> Result<Option<PathBuf>, ClientStateError> {
        if path.is_empty() {
            return Err(ClientStateError::Sqlite(
                "sqlite database path is empty".into(),
            ));
        }
        if path == ":memory:" || path == "memory" {
            return Ok(None);
        }
        Ok(Some(PathBuf::from(path)))
    }

    fn current_unix_ms_i64() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0)
    }
}

#[cfg(feature = "client-state-sqlite")]
pub use sqlite::SqliteClientStateStore;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ClientStateError {
    #[error("client state key is invalid: {0}")]
    InvalidKey(String),
    #[error("client state I/O failed: {0}")]
    Io(String),
    #[error("client state JSON failed: {0}")]
    Json(String),
    #[error("client state sqlite failed: {0}")]
    Sqlite(String),
}

impl ClientStateError {
    #[cfg(feature = "client-state-sqlite")]
    fn json(error: impl std::fmt::Display) -> Self {
        Self::Json(error.to_string())
    }

    #[cfg(feature = "client-state-sqlite")]
    fn sqlite(error: impl std::fmt::Display) -> Self {
        Self::Sqlite(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::ClientStateKey;
    #[cfg(feature = "client-state-sqlite")]
    use super::{ClientStateStore, SqliteClientStateStore};
    #[cfg(feature = "client-state-sqlite")]
    use serde_json::json;

    #[cfg(feature = "client-state-sqlite")]
    #[tokio::test]
    async fn sqlite_client_state_round_trips_json_values() {
        let store = SqliteClientStateStore::new(":memory:").unwrap();
        let key = ClientStateKey::new("telegram", "bot", "offset", "polling").unwrap();

        assert_eq!(store.get(&key).await.unwrap(), None);
        store.set(&key, &json!(42)).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(json!(42)));
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[test]
    fn client_state_key_rejects_empty_parts() {
        assert!(ClientStateKey::new("telegram", "", "offset", "polling").is_err());
    }
}
