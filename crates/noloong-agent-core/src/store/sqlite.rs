use super::{EventStore, StoreFuture};
use crate::{AgentCoreError, AgentEvent, AgentEventKind, Result, clock};
use std::path::{Path, PathBuf};
use toasty_driver_sqlite::Sqlite;

const MEMORY_URL: &str = "sqlite::memory:";
const TABLE_NAME: &str = "stored_agent_events";

#[derive(Clone, Debug)]
pub struct SqliteEventStoreConfig {
    pub database_url: String,
    pub migrate_on_connect: bool,
}

impl SqliteEventStoreConfig {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            migrate_on_connect: true,
        }
    }

    pub fn in_memory() -> Self {
        Self::new(MEMORY_URL)
    }

    pub fn file(path: impl AsRef<Path>) -> Self {
        Self::new(format!("sqlite:{}", path.as_ref().display()))
    }

    pub fn without_migrations(mut self) -> Self {
        self.migrate_on_connect = false;
        self
    }
}

#[derive(Clone)]
pub struct SqliteEventStore {
    db: toasty::Db,
}

impl SqliteEventStore {
    pub async fn connect(config: SqliteEventStoreConfig) -> Result<Self> {
        let location = SqliteLocation::parse(&config.database_url)?;
        validate_schema_state(&location, config.migrate_on_connect)?;

        let driver = location.driver();
        let db = toasty::Db::builder()
            .models(toasty::models!(StoredAgentEvent))
            .build(driver)
            .await
            .map_err(to_store_error)?;

        if should_push_schema(&location, config.migrate_on_connect)? {
            db.push_schema().await.map_err(to_store_error)?;
        }

        Ok(Self { db })
    }

    pub async fn in_memory() -> Result<Self> {
        Self::connect(SqliteEventStoreConfig::in_memory()).await
    }
}

impl EventStore for SqliteEventStore {
    fn append<'a>(&'a self, event: AgentEvent) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let kind_type = event_kind_type(&event.kind).to_string();
            let created_at_ms = current_time_ms()?;
            let event_json = serde_json::to_string(&event)?;
            let AgentEvent {
                run_id,
                sequence,
                turn_id,
                phase,
                kind: _,
            } = event;

            toasty::create!(StoredAgentEvent {
                run_id,
                sequence,
                turn_id,
                phase,
                kind_type,
                event_json,
                created_at_ms,
            })
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;

            Ok(())
        })
    }

    fn load<'a>(&'a self, run_id: &'a str) -> StoreFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let mut query = toasty::stmt::Query::<toasty::stmt::List<StoredAgentEvent>>::filter(
                StoredAgentEvent::fields().run_id().eq(run_id.to_string()),
            );
            query.order_by(StoredAgentEvent::fields().sequence().asc());

            let rows = query.exec(&mut db).await.map_err(to_store_error)?;
            rows.into_iter()
                .map(|row| decode_event(run_id, row.sequence, &row.event_json))
                .collect()
        })
    }
}

#[derive(Debug, toasty::Model)]
#[table = "stored_agent_events"]
#[key(partition = run_id, local = sequence)]
struct StoredAgentEvent {
    run_id: String,
    sequence: u64,
    turn_id: Option<u64>,
    phase: Option<String>,
    kind_type: String,
    event_json: String,
    created_at_ms: u64,
}

#[derive(Clone, Debug)]
enum SqliteLocation {
    InMemory,
    File(PathBuf),
}

impl SqliteLocation {
    fn parse(database_url: &str) -> Result<Self> {
        match database_url {
            "" => Err(AgentCoreError::Store("sqlite database url is empty".into())),
            MEMORY_URL | "sqlite://memory" | ":memory:" => Ok(Self::InMemory),
            url if url.starts_with("sqlite://") => {
                let path = url.strip_prefix("sqlite://").unwrap_or_default();
                if path.is_empty() {
                    Err(AgentCoreError::Store(
                        "sqlite database path is empty".into(),
                    ))
                } else if path == "memory" {
                    Ok(Self::InMemory)
                } else {
                    Ok(Self::File(PathBuf::from(path)))
                }
            }
            url if url.starts_with("sqlite:") => {
                let path = url.strip_prefix("sqlite:").unwrap_or_default();
                if path.is_empty() {
                    Err(AgentCoreError::Store(
                        "sqlite database path is empty".into(),
                    ))
                } else if path == ":memory:" {
                    Ok(Self::InMemory)
                } else {
                    Ok(Self::File(PathBuf::from(path)))
                }
            }
            path if path.contains("://") => Err(AgentCoreError::Store(format!(
                "unsupported sqlite database url scheme: {path}"
            ))),
            path => Ok(Self::File(PathBuf::from(path))),
        }
    }

    fn driver(&self) -> Sqlite {
        match self {
            Self::InMemory => Sqlite::in_memory(),
            Self::File(path) => Sqlite::open(path),
        }
    }
}

fn validate_schema_state(location: &SqliteLocation, migrate_on_connect: bool) -> Result<()> {
    if migrate_on_connect {
        return Ok(());
    }

    match location {
        SqliteLocation::InMemory => Err(AgentCoreError::Store(
            "sqlite in-memory store requires migrate_on_connect=true".into(),
        )),
        SqliteLocation::File(path) if sqlite_schema_exists(path)? => Ok(()),
        SqliteLocation::File(path) => Err(AgentCoreError::Store(format!(
            "sqlite event store schema is missing at {}; enable migrate_on_connect",
            path.display()
        ))),
    }
}

fn should_push_schema(location: &SqliteLocation, migrate_on_connect: bool) -> Result<bool> {
    if !migrate_on_connect {
        return Ok(false);
    }

    match location {
        SqliteLocation::InMemory => Ok(true),
        SqliteLocation::File(path) => Ok(!sqlite_schema_exists(path)?),
    }
}

fn sqlite_schema_exists(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(to_store_error)?;
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [TABLE_NAME],
            |row| row.get(0),
        )
        .map_err(to_store_error)?;
    Ok(count > 0)
}

fn decode_event(run_id: &str, sequence: u64, event_json: &str) -> Result<AgentEvent> {
    serde_json::from_str(event_json).map_err(|error| {
        AgentCoreError::Store(format!(
            "failed to decode event run_id={run_id} sequence={sequence}: {error}"
        ))
    })
}

fn event_kind_type(kind: &AgentEventKind) -> &'static str {
    match kind {
        AgentEventKind::RunStarted => "run_started",
        AgentEventKind::RunCompleted => "run_completed",
        AgentEventKind::RunAborted => "run_aborted",
        AgentEventKind::RunFailed { .. } => "run_failed",
        AgentEventKind::TurnStarted => "turn_started",
        AgentEventKind::TurnCompleted { .. } => "turn_completed",
        AgentEventKind::PhaseStarted { .. } => "phase_started",
        AgentEventKind::PhaseCompleted { .. } => "phase_completed",
        AgentEventKind::PhaseFailed { .. } => "phase_failed",
        AgentEventKind::EffectProposed { .. } => "effect_proposed",
        AgentEventKind::EffectCommitted { .. } => "effect_committed",
        AgentEventKind::EffectRejected { .. } => "effect_rejected",
        AgentEventKind::ModelStreamEvent { .. } => "model_stream_event",
        AgentEventKind::ToolCallResolved { .. } => "tool_call_resolved",
        AgentEventKind::ToolPermissionRequested { .. } => "tool_permission_requested",
        AgentEventKind::ToolPermissionDecided { .. } => "tool_permission_decided",
        AgentEventKind::ToolApprovalRequested { .. } => "tool_approval_requested",
        AgentEventKind::ToolApprovalResolved { .. } => "tool_approval_resolved",
        AgentEventKind::ToolApprovalExpired { .. } => "tool_approval_expired",
        AgentEventKind::ToolExecutionStarted { .. } => "tool_execution_started",
        AgentEventKind::ToolExecutionUpdate { .. } => "tool_execution_update",
        AgentEventKind::ToolExecutionCompleted { .. } => "tool_execution_completed",
        AgentEventKind::RunPaused { .. } => "run_paused",
        AgentEventKind::RunResumed { .. } => "run_resumed",
        AgentEventKind::ExtensionEvent { .. } => "extension_event",
    }
}

fn current_time_ms() -> Result<u64> {
    clock::current_unix_ms().map_err(|error| {
        AgentCoreError::Store(format!("system clock is before unix epoch: {error}"))
    })
}

fn to_store_error(error: impl std::error::Error) -> AgentCoreError {
    AgentCoreError::Store(error.to_string())
}
