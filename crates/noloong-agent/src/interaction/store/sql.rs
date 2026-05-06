use super::codec::{decode_record_json, encode_record_json};
use super::{AgentSessionRecord, AgentSessionRegistryStore, duplicate_session_error};
use crate::interaction::{InteractionError, InteractionFuture};
use noloong_agent_core::RunStatus;
#[cfg(feature = "registry-store-sqlite")]
use std::path::{Path, PathBuf};
use toasty::stmt::{List, Query};

#[cfg(feature = "registry-store-postgres")]
use toasty_driver_postgresql::PostgreSQL;
#[cfg(feature = "registry-store-sqlite")]
use toasty_driver_sqlite::Sqlite;

#[derive(Clone, Debug)]
pub struct SqlAgentSessionRegistryStoreConfig {
    pub database_url: String,
    pub migrate_on_connect: bool,
    pub table_name_prefix: Option<String>,
}

impl SqlAgentSessionRegistryStoreConfig {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            migrate_on_connect: true,
            table_name_prefix: None,
        }
    }

    #[cfg(feature = "registry-store-sqlite")]
    pub fn sqlite_in_memory() -> Self {
        Self::new("sqlite::memory:")
    }

    #[cfg(feature = "registry-store-sqlite")]
    pub fn sqlite_file(path: impl AsRef<Path>) -> Self {
        Self::new(format!("sqlite:{}", path.as_ref().display()))
    }

    pub fn without_migrations(mut self) -> Self {
        self.migrate_on_connect = false;
        self
    }

    pub fn with_table_name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.table_name_prefix = Some(prefix.into());
        self
    }
}

#[derive(Clone)]
pub struct SqlAgentSessionRegistryStore {
    db: toasty::Db,
}

impl SqlAgentSessionRegistryStore {
    pub async fn connect(
        config: SqlAgentSessionRegistryStoreConfig,
    ) -> Result<Self, InteractionError> {
        let location = SqlStoreLocation::parse(&config.database_url)?;
        let db = location
            .build_db(config.table_name_prefix.as_deref())
            .await?;
        if config.migrate_on_connect {
            db.push_schema().await.map_err(to_store_error)?;
        }
        Ok(Self { db })
    }
}

impl AgentSessionRegistryStore for SqlAgentSessionRegistryStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self.get(&record.session_id).await?.is_some() {
                return Err(duplicate_session_error(&record.session_id));
            }
            let mut db = self.db.clone();
            let row = row_from_record(record)?;
            toasty::create!(StoredAgentSession {
                session_id: row.session_id,
                profile_id: row.profile_id,
                parent_session_id: row.parent_session_id,
                role: row.role,
                status: row.status,
                record_json: row.record_json,
                created_at_ms: row.created_at_ms,
                updated_at_ms: row.updated_at_ms,
            })
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self.get(&record.session_id).await?.is_none() {
                return Err(super::missing_session_error(&record.session_id));
            }
            let mut db = self.db.clone();
            let row = row_from_record(record)?;
            StoredAgentSession::filter_by_session_id(row.session_id)
                .update()
                .profile_id(row.profile_id)
                .parent_session_id(row.parent_session_id)
                .role(row.role)
                .status(row.status)
                .record_json(row.record_json)
                .created_at_ms(row.created_at_ms)
                .updated_at_ms(row.updated_at_ms)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut db = self.db.clone();
            session_query(session_id)
                .delete()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = session_query(session_id)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter().next().map(decode_row).transpose()
        })
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = Query::<List<StoredAgentSession>>::all()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter().map(decode_row).collect()
        })
    }
}

#[derive(Debug, toasty::Model)]
#[table = "stored_agent_sessions"]
struct StoredAgentSession {
    #[key]
    session_id: String,
    profile_id: String,
    parent_session_id: Option<String>,
    role: Option<String>,
    status: String,
    record_json: String,
    created_at_ms: u64,
    updated_at_ms: u64,
}

enum SqlStoreLocation {
    #[cfg(feature = "registry-store-sqlite")]
    SqliteInMemory,
    #[cfg(feature = "registry-store-sqlite")]
    SqliteFile(PathBuf),
    #[cfg(feature = "registry-store-postgres")]
    Postgres(String),
}

impl SqlStoreLocation {
    fn parse(database_url: &str) -> Result<Self, InteractionError> {
        match database_url {
            "" => Err(InteractionError::internal("sql database url is empty")),
            #[cfg(feature = "registry-store-sqlite")]
            "sqlite::memory:" | "sqlite://memory" | ":memory:" => Ok(Self::SqliteInMemory),
            #[cfg(feature = "registry-store-sqlite")]
            url if url.starts_with("sqlite://") => sqlite_path(url, "sqlite://"),
            #[cfg(feature = "registry-store-sqlite")]
            url if url.starts_with("sqlite:") => sqlite_path(url, "sqlite:"),
            #[cfg(feature = "registry-store-postgres")]
            url if url.starts_with("postgres://") || url.starts_with("postgresql://") => {
                Ok(Self::Postgres(url.to_owned()))
            }
            url if url.contains("://") => Err(InteractionError::internal(format!(
                "unsupported sql database url scheme: {url}"
            ))),
            #[cfg(feature = "registry-store-sqlite")]
            path => Ok(Self::SqliteFile(PathBuf::from(path))),
            #[cfg(not(feature = "registry-store-sqlite"))]
            path => Err(InteractionError::internal(format!(
                "unsupported sql database url without sqlite feature: {path}"
            ))),
        }
    }

    async fn build_db(
        &self,
        table_name_prefix: Option<&str>,
    ) -> Result<toasty::Db, InteractionError> {
        let mut builder = toasty::Db::builder();
        builder.models(toasty::models!(StoredAgentSession));
        if let Some(prefix) = table_name_prefix {
            builder.table_name_prefix(prefix);
        }
        match self {
            #[cfg(feature = "registry-store-sqlite")]
            Self::SqliteInMemory => builder
                .build(Sqlite::in_memory())
                .await
                .map_err(to_store_error),
            #[cfg(feature = "registry-store-sqlite")]
            Self::SqliteFile(path) => builder
                .build(Sqlite::open(path))
                .await
                .map_err(to_store_error),
            #[cfg(feature = "registry-store-postgres")]
            Self::Postgres(url) => builder
                .build(PostgreSQL::new(url).map_err(to_store_error)?)
                .await
                .map_err(to_store_error),
        }
    }
}

#[cfg(feature = "registry-store-sqlite")]
fn sqlite_path(url: &str, prefix: &str) -> Result<SqlStoreLocation, InteractionError> {
    let path = url.strip_prefix(prefix).unwrap_or_default();
    if path.is_empty() {
        Err(InteractionError::internal("sqlite database path is empty"))
    } else if path == ":memory:" || path == "memory" {
        Ok(SqlStoreLocation::SqliteInMemory)
    } else {
        Ok(SqlStoreLocation::SqliteFile(PathBuf::from(path)))
    }
}

fn row_from_record(record: AgentSessionRecord) -> Result<StoredAgentSession, InteractionError> {
    let status = run_status_type(&record.state.status).to_string();
    let record_json = encode_record_json(&record)?;
    Ok(StoredAgentSession {
        session_id: record.session_id,
        profile_id: record.profile_id,
        parent_session_id: record.parent_session_id,
        role: record.role,
        status,
        record_json,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
    })
}

fn decode_row(row: StoredAgentSession) -> Result<AgentSessionRecord, InteractionError> {
    let record = decode_record_json(&row.session_id, row.record_json.as_bytes())?;
    validate_row_consistency(&row, &record)?;
    Ok(record)
}

fn validate_row_consistency(
    row: &StoredAgentSession,
    record: &AgentSessionRecord,
) -> Result<(), InteractionError> {
    let status = run_status_type(&record.state.status);
    let consistent = row.session_id.as_str() == record.session_id.as_str()
        && row.profile_id.as_str() == record.profile_id.as_str()
        && row.parent_session_id.as_deref() == record.parent_session_id.as_deref()
        && row.role.as_deref() == record.role.as_deref()
        && row.status.as_str() == status
        && row.created_at_ms == record.created_at_ms
        && row.updated_at_ms == record.updated_at_ms;
    if consistent {
        Ok(())
    } else {
        Err(InteractionError::internal(format!(
            "stored session record metadata drift detected: {}",
            row.session_id
        )))
    }
}

fn session_query(session_id: &str) -> Query<List<StoredAgentSession>> {
    Query::<List<StoredAgentSession>>::filter(
        StoredAgentSession::fields()
            .session_id()
            .eq(session_id.to_string()),
    )
}

fn run_status_type(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Idle => "idle",
        RunStatus::Running => "running",
        RunStatus::Completed => "completed",
        RunStatus::Aborted => "aborted",
        RunStatus::Failed => "failed",
        RunStatus::Paused => "paused",
    }
}

fn to_store_error(error: impl std::fmt::Display) -> InteractionError {
    InteractionError::internal(format!("sql agent session registry store error: {error}"))
}
