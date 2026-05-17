use super::codec::{decode_record_json, encode_record_json};
use super::{
    AgentSessionRecord, AgentSessionRegistryStore, AutomationRecord, AutomationScheduleScan,
    AutomationScheduleScanBuilder, GoalRecord, duplicate_automation_error, duplicate_session_error,
    missing_automation_error, record_matches_session_list_filter, session_metadata_index_value,
};
use crate::interaction::{
    AgentSessionListFilter, AutomationStatus, GoalStatus, InteractionError, InteractionFuture,
};
use noloong_agent_core::RunStatus;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
#[cfg(feature = "registry-store-sqlite")]
use std::path::{Path, PathBuf};
use toasty::stmt::{List, Query};

#[cfg(feature = "registry-store-postgres")]
use toasty_driver_postgresql::PostgreSQL;
#[cfg(feature = "registry-store-sqlite")]
use toasty_driver_sqlite::Sqlite;

#[cfg(feature = "registry-store-sqlite")]
const SQLITE_REGISTRY_TABLES: &[&str] = &[
    "stored_agent_sessions",
    "stored_agent_session_metadata",
    "stored_goals",
    "stored_automations",
];

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
        validate_schema_state(
            &location,
            config.migrate_on_connect,
            config.table_name_prefix.as_deref(),
        )?;
        let db = location
            .build_db(config.table_name_prefix.as_deref())
            .await?;
        if should_push_schema(
            &location,
            config.migrate_on_connect,
            config.table_name_prefix.as_deref(),
        )? {
            db.push_schema().await.map_err(to_store_error)?;
        }
        Ok(Self { db })
    }

    async fn save_session_metadata_index(
        &self,
        session_id: &str,
        metadata: &Map<String, Value>,
    ) -> Result<(), InteractionError> {
        let mut db = self.db.clone();
        session_metadata_query_for_session(session_id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
        for (key, value) in metadata {
            let Some(metadata_value) = session_metadata_index_value(value) else {
                continue;
            };
            toasty::create!(StoredAgentSessionMetadata {
                metadata_id: session_metadata_id(session_id, key),
                session_id: session_id.to_owned(),
                metadata_key: key.clone(),
                metadata_value,
            })
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
        }
        Ok(())
    }

    async fn remove_session_metadata_index(
        &self,
        session_id: &str,
    ) -> Result<(), InteractionError> {
        let mut db = self.db.clone();
        session_metadata_query_for_session(session_id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
        Ok(())
    }

    async fn metadata_session_candidates(
        &self,
        filter: &AgentSessionListFilter,
    ) -> Result<Option<BTreeSet<String>>, InteractionError> {
        if filter.metadata_equals.is_empty() {
            return Ok(None);
        }
        let mut candidate_ids: Option<BTreeSet<String>> = None;
        for (key, value) in &filter.metadata_equals {
            let Some(metadata_value) = session_metadata_index_value(value) else {
                return Ok(Some(BTreeSet::new()));
            };
            let mut db = self.db.clone();
            let rows = Query::<List<StoredAgentSessionMetadata>>::filter(
                StoredAgentSessionMetadata::fields()
                    .metadata_key()
                    .eq(key.clone())
                    .and(
                        StoredAgentSessionMetadata::fields()
                            .metadata_value()
                            .eq(metadata_value),
                    ),
            )
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
            let matching_ids = rows
                .into_iter()
                .map(|row| row.session_id)
                .collect::<BTreeSet<_>>();
            candidate_ids = Some(match candidate_ids {
                Some(existing) => existing
                    .intersection(&matching_ids)
                    .cloned()
                    .collect::<BTreeSet<_>>(),
                None => matching_ids,
            });
            if candidate_ids.as_ref().is_some_and(BTreeSet::is_empty) {
                break;
            }
        }
        Ok(candidate_ids)
    }
}

impl AgentSessionRegistryStore for SqlAgentSessionRegistryStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self.get(&record.session_id).await?.is_some() {
                return Err(duplicate_session_error(&record.session_id));
            }
            let metadata = record.metadata.clone();
            let mut db = self.db.clone();
            let row = row_from_record(record)?;
            let session_id = row.session_id.clone();
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
            self.save_session_metadata_index(&session_id, &metadata)
                .await?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self.get(&record.session_id).await?.is_none() {
                return Err(super::missing_session_error(&record.session_id));
            }
            let metadata = record.metadata.clone();
            let mut db = self.db.clone();
            let row = row_from_record(record)?;
            let session_id = row.session_id.clone();
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
            self.save_session_metadata_index(&session_id, &metadata)
                .await?;
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
            self.remove_session_metadata_index(session_id).await?;
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

    fn list<'a>(
        &'a self,
        filter: &'a AgentSessionListFilter,
    ) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            let rows = if let Some(candidate_ids) = self.metadata_session_candidates(filter).await?
            {
                let mut rows = Vec::new();
                for session_id in candidate_ids {
                    let mut db = self.db.clone();
                    rows.extend(
                        session_query(&session_id)
                            .exec(&mut db)
                            .await
                            .map_err(to_store_error)?,
                    );
                }
                rows
            } else {
                let mut db = self.db.clone();
                Query::<List<StoredAgentSession>>::all()
                    .exec(&mut db)
                    .await
                    .map_err(to_store_error)?
            };
            rows.into_iter()
                .filter(|row| session_row_matches_filter(row, filter))
                .map(decode_row)
                .filter_map(|record| match record {
                    Ok(record) if record_matches_session_list_filter(&record, filter) => {
                        Some(Ok(record))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        })
    }

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let row = goal_row(goal)?;
            if goal_query(&row.session_id)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?
                .into_iter()
                .next()
                .is_some()
            {
                StoredGoal::filter_by_session_id(row.session_id)
                    .update()
                    .goal_id(row.goal_id)
                    .status(row.status)
                    .record_json(row.record_json)
                    .updated_at_ms(row.updated_at_ms)
                    .exec(&mut db)
                    .await
                    .map_err(to_store_error)?;
            } else {
                toasty::create!(StoredGoal {
                    session_id: row.session_id,
                    goal_id: row.goal_id,
                    status: row.status,
                    record_json: row.record_json,
                    updated_at_ms: row.updated_at_ms,
                })
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            }
            Ok(())
        })
    }

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = goal_query(session_id)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter().next().map(decode_goal_row).transpose()
        })
    }

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = Query::<List<StoredGoal>>::all()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter().map(decode_goal_row).collect()
        })
    }

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut db = self.db.clone();
            goal_query(session_id)
                .delete()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self
                .get_automation(&automation.automation_id)
                .await?
                .is_some()
            {
                return Err(duplicate_automation_error(&automation.automation_id));
            }
            let mut db = self.db.clone();
            let row = automation_row(automation)?;
            toasty::create!(StoredAutomation {
                automation_id: row.automation_id,
                status: row.status,
                next_fire_at_ms: row.next_fire_at_ms,
                record_json: row.record_json,
                updated_at_ms: row.updated_at_ms,
            })
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            if self
                .get_automation(&automation.automation_id)
                .await?
                .is_none()
            {
                return Err(missing_automation_error(&automation.automation_id));
            }
            let mut db = self.db.clone();
            let row = automation_row(automation)?;
            StoredAutomation::filter_by_automation_id(row.automation_id)
                .update()
                .status(row.status)
                .next_fire_at_ms(row.next_fire_at_ms)
                .record_json(row.record_json)
                .updated_at_ms(row.updated_at_ms)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = automation_query(automation_id)
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter()
                .next()
                .map(decode_automation_row)
                .transpose()
        })
    }

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let rows = Query::<List<StoredAutomation>>::all()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            rows.into_iter().map(decode_automation_row).collect()
        })
    }

    fn scan_automation_schedule<'a>(
        &'a self,
        now_ms: u64,
    ) -> InteractionFuture<'a, AutomationScheduleScan> {
        Box::pin(async move {
            let mut db = self.db.clone();
            let active_status = automation_status_type(&AutomationStatus::Active).to_string();
            let due_rows = Query::<List<StoredAutomation>>::filter(
                StoredAutomation::fields()
                    .status()
                    .eq(active_status.clone())
                    .and(
                        StoredAutomation::fields()
                            .next_fire_at_ms()
                            .le(Some(now_ms)),
                    ),
            )
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
            let mut scan = AutomationScheduleScanBuilder::default();
            for row in due_rows {
                scan.include(row.automation_id, true, row.next_fire_at_ms, now_ms);
            }

            let future_rows = Query::<List<StoredAutomation>>::filter(
                StoredAutomation::fields().status().eq(active_status).and(
                    StoredAutomation::fields()
                        .next_fire_at_ms()
                        .gt(Some(now_ms)),
                ),
            )
            .exec(&mut db)
            .await
            .map_err(to_store_error)?;
            for row in future_rows {
                scan.include(row.automation_id, true, row.next_fire_at_ms, now_ms);
            }

            Ok(scan.finish())
        })
    }

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut db = self.db.clone();
            automation_query(automation_id)
                .delete()
                .exec(&mut db)
                .await
                .map_err(to_store_error)?;
            Ok(())
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

#[derive(Debug, toasty::Model)]
#[table = "stored_agent_session_metadata"]
struct StoredAgentSessionMetadata {
    #[key]
    metadata_id: String,
    session_id: String,
    metadata_key: String,
    metadata_value: String,
}

#[derive(Debug, toasty::Model)]
#[table = "stored_goals"]
struct StoredGoal {
    #[key]
    session_id: String,
    goal_id: String,
    status: String,
    record_json: String,
    updated_at_ms: u64,
}

#[derive(Debug, toasty::Model)]
#[table = "stored_automations"]
struct StoredAutomation {
    #[key]
    automation_id: String,
    status: String,
    next_fire_at_ms: Option<u64>,
    record_json: String,
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
        builder.models(toasty::models!(
            StoredAgentSession,
            StoredAgentSessionMetadata,
            StoredGoal,
            StoredAutomation
        ));
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

fn validate_schema_state(
    location: &SqlStoreLocation,
    migrate_on_connect: bool,
    table_name_prefix: Option<&str>,
) -> Result<(), InteractionError> {
    if migrate_on_connect {
        return Ok(());
    }

    match location {
        #[cfg(feature = "registry-store-sqlite")]
        SqlStoreLocation::SqliteInMemory => Err(InteractionError::internal(
            "sqlite in-memory registry store requires migrate_on_connect=true",
        )),
        #[cfg(feature = "registry-store-sqlite")]
        SqlStoreLocation::SqliteFile(path)
            if sqlite_registry_schema_exists(path, table_name_prefix)? =>
        {
            Ok(())
        }
        #[cfg(feature = "registry-store-sqlite")]
        SqlStoreLocation::SqliteFile(path) => Err(InteractionError::internal(format!(
            "sqlite registry store schema is missing at {}; enable migrate_on_connect",
            path.display()
        ))),
        #[cfg(feature = "registry-store-postgres")]
        SqlStoreLocation::Postgres(_) => Ok(()),
    }
}

fn should_push_schema(
    location: &SqlStoreLocation,
    migrate_on_connect: bool,
    table_name_prefix: Option<&str>,
) -> Result<bool, InteractionError> {
    if !migrate_on_connect {
        return Ok(false);
    }

    match location {
        #[cfg(feature = "registry-store-sqlite")]
        SqlStoreLocation::SqliteInMemory => Ok(true),
        #[cfg(feature = "registry-store-sqlite")]
        SqlStoreLocation::SqliteFile(path) => {
            Ok(!sqlite_registry_schema_exists(path, table_name_prefix)?)
        }
        #[cfg(feature = "registry-store-postgres")]
        SqlStoreLocation::Postgres(_) => Ok(true),
    }
}

#[cfg(feature = "registry-store-sqlite")]
fn sqlite_registry_schema_exists(
    path: &Path,
    table_name_prefix: Option<&str>,
) -> Result<bool, InteractionError> {
    if !path.exists() {
        return Ok(false);
    }

    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(to_store_error)?;
    for table in SQLITE_REGISTRY_TABLES {
        let table_name = registry_table_name(table_name_prefix, table);
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table_name],
                |row| row.get(0),
            )
            .map_err(to_store_error)?;
        if count == 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(feature = "registry-store-sqlite")]
fn registry_table_name(table_name_prefix: Option<&str>, table: &str) -> String {
    match table_name_prefix {
        Some(prefix) => format!("{prefix}{table}"),
        None => table.to_owned(),
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

fn session_row_matches_filter(row: &StoredAgentSession, filter: &AgentSessionListFilter) -> bool {
    if filter
        .parent_session_id
        .as_ref()
        .is_some_and(|parent| row.parent_session_id.as_ref() != Some(parent))
    {
        return false;
    }
    if filter
        .profile_id
        .as_ref()
        .is_some_and(|profile| &row.profile_id != profile)
    {
        return false;
    }
    if filter
        .status
        .as_ref()
        .is_some_and(|status| row.status != status.as_str())
    {
        return false;
    }
    true
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

fn session_metadata_query_for_session(session_id: &str) -> Query<List<StoredAgentSessionMetadata>> {
    Query::<List<StoredAgentSessionMetadata>>::filter(
        StoredAgentSessionMetadata::fields()
            .session_id()
            .eq(session_id.to_string()),
    )
}

fn session_metadata_id(session_id: &str, key: &str) -> String {
    serde_json::to_string(&(session_id, key)).expect("metadata id tuple serializes")
}

fn goal_query(session_id: &str) -> Query<List<StoredGoal>> {
    Query::<List<StoredGoal>>::filter(StoredGoal::fields().session_id().eq(session_id.to_string()))
}

fn automation_query(automation_id: &str) -> Query<List<StoredAutomation>> {
    Query::<List<StoredAutomation>>::filter(
        StoredAutomation::fields()
            .automation_id()
            .eq(automation_id.to_string()),
    )
}

fn goal_row(goal: GoalRecord) -> Result<StoredGoal, InteractionError> {
    let status = goal_status_type(&goal.status).to_string();
    let record_json = serde_json::to_string(&goal).map_err(to_store_error)?;
    Ok(StoredGoal {
        session_id: goal.session_id,
        goal_id: goal.goal_id,
        status,
        record_json,
        updated_at_ms: goal.updated_at_ms,
    })
}

fn decode_goal_row(row: StoredGoal) -> Result<GoalRecord, InteractionError> {
    let record: GoalRecord = serde_json::from_str(&row.record_json).map_err(to_store_error)?;
    let status = goal_status_type(&record.status);
    let consistent = row.session_id == record.session_id
        && row.goal_id == record.goal_id
        && row.status == status
        && row.updated_at_ms == record.updated_at_ms;
    if consistent {
        Ok(record)
    } else {
        Err(InteractionError::internal(format!(
            "stored goal record metadata drift detected: {}",
            row.session_id
        )))
    }
}

fn automation_row(automation: AutomationRecord) -> Result<StoredAutomation, InteractionError> {
    let status = automation_status_type(&automation.status).to_string();
    let record_json = serde_json::to_string(&automation).map_err(to_store_error)?;
    Ok(StoredAutomation {
        automation_id: automation.automation_id,
        status,
        next_fire_at_ms: automation.next_fire_at_ms,
        record_json,
        updated_at_ms: automation.updated_at_ms,
    })
}

fn decode_automation_row(row: StoredAutomation) -> Result<AutomationRecord, InteractionError> {
    let record: AutomationRecord =
        serde_json::from_str(&row.record_json).map_err(to_store_error)?;
    let status = automation_status_type(&record.status);
    let consistent = row.automation_id == record.automation_id
        && row.status == status
        && row.next_fire_at_ms == record.next_fire_at_ms
        && row.updated_at_ms == record.updated_at_ms;
    if consistent {
        Ok(record)
    } else {
        Err(InteractionError::internal(format!(
            "stored automation record metadata drift detected: {}",
            row.automation_id
        )))
    }
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

fn goal_status_type(status: &GoalStatus) -> &'static str {
    match status {
        GoalStatus::Pursuing => "pursuing",
        GoalStatus::Paused => "paused",
        GoalStatus::Achieved => "achieved",
        GoalStatus::Unmet => "unmet",
        GoalStatus::BudgetLimited => "budget_limited",
        GoalStatus::Cleared => "cleared",
    }
}

fn automation_status_type(status: &AutomationStatus) -> &'static str {
    match status {
        AutomationStatus::Active => "active",
        AutomationStatus::Paused => "paused",
        AutomationStatus::Completed => "completed",
    }
}

fn to_store_error(error: impl std::fmt::Display) -> InteractionError {
    InteractionError::internal(format!("sql agent session registry store error: {error}"))
}
