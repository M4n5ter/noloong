#![cfg(feature = "sqlite-store")]

pub mod support;

use noloong_agent_core::*;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use support::core::{approval_runtime, native_runtime};

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn connect_in_memory_sqlite_store() -> Result<()> {
    let store = SqliteEventStore::in_memory().await?;

    assert!(store.load("missing-run").await?.is_empty());

    Ok(())
}

#[tokio::test]
async fn connect_file_sqlite_store() -> Result<()> {
    let db = TempSqliteDb::new("connect-file");
    let store = SqliteEventStore::connect(db.config()).await?;

    store
        .append(event("file-run", 1, AgentEventKind::RunStarted))
        .await?;
    let events = store.load("file-run").await?;

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted));

    Ok(())
}

#[tokio::test]
async fn missing_schema_without_migrations_returns_store_error() -> Result<()> {
    let db = TempSqliteDb::new("missing-schema");
    assert!(!db.path.exists());

    let error = match SqliteEventStore::connect(db.existing_config()).await {
        Ok(_) => panic!("connect should fail without schema"),
        Err(error) => error,
    };

    assert!(matches!(error, AgentCoreError::Store(_)));
    assert!(!db.path.exists());

    Ok(())
}

#[tokio::test]
async fn unsupported_database_url_scheme_is_rejected() -> Result<()> {
    let error =
        match SqliteEventStore::connect(SqliteEventStoreConfig::new("postgres://localhost/events"))
            .await
        {
            Ok(_) => panic!("connect should reject unsupported schemes"),
            Err(error) => error,
        };

    assert!(matches!(error, AgentCoreError::Store(_)));

    Ok(())
}

#[tokio::test]
async fn append_and_load_orders_by_sequence() -> Result<()> {
    let store = SqliteEventStore::in_memory().await?;

    store
        .append(event(
            "ordered-run",
            2,
            AgentEventKind::PhaseCompleted {
                phase: "second".into(),
            },
        ))
        .await?;
    store
        .append(event(
            "ordered-run",
            1,
            AgentEventKind::PhaseStarted {
                phase: "first".into(),
            },
        ))
        .await?;

    let events = store.load("ordered-run").await?;

    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(
        events[0].kind,
        AgentEventKind::PhaseStarted { ref phase } if phase == "first"
    ));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::PhaseCompleted { ref phase } if phase == "second"
    ));

    Ok(())
}

#[tokio::test]
async fn duplicate_sequence_is_rejected() -> Result<()> {
    let store = SqliteEventStore::in_memory().await?;
    let first = event("duplicate-run", 1, AgentEventKind::RunStarted);

    store.append(first.clone()).await?;
    let error = store
        .append(first)
        .await
        .expect_err("duplicate sequence should fail");

    assert!(matches!(error, AgentCoreError::Store(_)));

    Ok(())
}

#[tokio::test]
async fn durable_replay_survives_store_reconnect() -> Result<()> {
    let db = TempSqliteDb::new("durable-replay");
    let first_store: Arc<dyn EventStore> = Arc::new(SqliteEventStore::connect(db.config()).await?);
    let runtime = native_runtime().with_event_store(first_store).build()?;
    let report = runtime.run("hello").await?;

    let second_store = SqliteEventStore::connect(db.existing_config()).await?;
    let events = second_store.load(&report.run_id).await?;
    let replayed = reduce_events(&events)?;

    assert!(!events.is_empty());
    assert_eq!(replayed, report.state);

    Ok(())
}

#[tokio::test]
async fn approval_resume_survives_runtime_restart() -> Result<()> {
    let db = TempSqliteDb::new("approval-resume");
    let first_store: Arc<dyn EventStore> = Arc::new(SqliteEventStore::connect(db.config()).await?);
    let fast_calls = Arc::new(AtomicU64::new(0));
    let runtime = approval_runtime(first_store, Arc::clone(&fast_calls), None)?;

    let paused = runtime.run("tools").await?;

    assert_eq!(fast_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(paused.state.status, RunStatus::Paused));
    assert_eq!(paused.state.pending_tool_approvals.len(), 1);

    let approval_id = paused
        .state
        .pending_tool_approvals
        .keys()
        .next()
        .expect("approval should be pending")
        .clone();

    let second_store: Arc<dyn EventStore> =
        Arc::new(SqliteEventStore::connect(db.existing_config()).await?);
    let restarted_runtime = approval_runtime(second_store, Arc::clone(&fast_calls), None)?;
    let resumed = restarted_runtime
        .resume_tool_approvals(
            &paused.run_id,
            vec![ToolApprovalResolution {
                approval_id: approval_id.clone(),
                decision: ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("approved by sqlite test".into()),
                    approver: Some("human".into()),
                    metadata: json!({ "ticket": "SQLITE-1" }),
                },
            }],
            None,
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(resumed.run_id, paused.run_id);
    assert_eq!(fast_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(resumed.state.status, RunStatus::Completed));
    assert!(resumed.state.pending_tool_approvals.is_empty());
    assert_eq!(reduce_events(&resumed.events)?, resumed.state);
    assert!(resumed.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolApprovalResolved {
            approval_id: event_approval_id,
            decision,
        } if event_approval_id == &approval_id
            && decision.outcome == ToolPermissionOutcome::Allow)
    }));
    assert!(
        resumed
            .events
            .iter()
            .any(|event| matches!(&event.kind, AgentEventKind::RunResumed { .. }))
    );
    assert!(resumed.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolExecutionCompleted { tool_call_id, output }
            if tool_call_id == "fast-call" && !output.is_error)
    }));

    Ok(())
}

fn event(run_id: &str, sequence: u64, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        sequence,
        run_id: run_id.to_string(),
        turn_id: None,
        phase: None,
        kind,
    }
}

struct TempSqliteDb {
    path: PathBuf,
}

impl TempSqliteDb {
    fn new(name: &str) -> Self {
        let id = NEXT_DB_ID.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "noloong-agent-core-{name}-{}-{id}.sqlite",
            std::process::id()
        ));
        Self { path }
    }

    fn config(&self) -> SqliteEventStoreConfig {
        SqliteEventStoreConfig::file(&self.path)
    }

    fn existing_config(&self) -> SqliteEventStoreConfig {
        self.config().without_migrations()
    }
}

impl Drop for TempSqliteDb {
    fn drop(&mut self) {
        remove_if_exists(&self.path);
        remove_if_exists(&self.path.with_extension("sqlite-shm"));
        remove_if_exists(&self.path.with_extension("sqlite-wal"));
    }
}

fn remove_if_exists(path: &Path) {
    let _ = std::fs::remove_file(path);
}
