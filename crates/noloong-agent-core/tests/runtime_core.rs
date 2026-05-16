pub mod support;

use noloong_agent_core::*;
use serde_json::json;
use std::sync::{Arc, atomic::AtomicU64};
use support::core::*;
use tokio::sync::Mutex;
#[tokio::test]
async fn phase_graph_allows_inserting_effectful_phase() -> Result<()> {
    let runtime = native_runtime()
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(InsertedPhase))
        .build()?;

    let report = runtime.run("hello").await?;

    assert_eq!(report.state.context.get("inserted"), Some(&json!(true)));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseStarted { phase } if phase == "test.inserted"
        )
    }));
    Ok(())
}

#[tokio::test]
async fn invalid_effect_is_rejected_and_fails_run() -> Result<()> {
    let runtime = native_runtime()
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(InvalidEffectPhase))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();
    assert!(matches!(error, AgentCoreError::InvalidEffect(_)));
    Ok(())
}

#[tokio::test]
async fn run_with_events_emits_realtime_events_in_order() -> Result<()> {
    let runtime = native_runtime().build()?;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&events);

    runtime
        .run_with_events("hello", move |event| {
            let received = Arc::clone(&received);
            async move {
                received.lock().await.push(event.kind);
                Ok(())
            }
        })
        .await?;

    let events = events.lock().await;
    assert!(matches!(events.first(), Some(AgentEventKind::RunStarted)));
    assert!(matches!(events.get(1), Some(AgentEventKind::TurnStarted)));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEventKind::ModelStreamEvent { .. }))
    );
    assert!(matches!(events.last(), Some(AgentEventKind::RunCompleted)));
    Ok(())
}

#[tokio::test]
async fn runtime_run_id_prefix_namespaces_event_log() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = native_runtime()
        .with_event_store(event_store.clone())
        .with_run_id_prefix("session:root/1")
        .build()?;

    let report = runtime.run("hello").await?;

    assert_eq!(report.state.run_id.as_deref(), Some("run-session-root-1-1"));
    assert!(!event_store.load("run-session-root-1-1").await?.is_empty());
    assert!(event_store.load("run-1").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn event_sink_failure_records_run_failed() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = native_runtime()
        .with_event_store(event_store.clone())
        .build()?;

    let error = runtime
        .run_with_events("hello", |event| async move {
            if matches!(event.kind, AgentEventKind::TurnStarted) {
                Err(AgentCoreError::EventSink("boom".into()))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap_err();

    assert!(matches!(error, AgentCoreError::EventSink(_)));
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn model_stream_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(FailingModel))
        .build()?;

    let error = runtime.run("fail").await.unwrap_err();

    assert!(error.to_string().contains("model stream failed"));
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::Failed { .. },
                ..
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn context_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(NativeModel {
            calls: AtomicU64::new(0),
        }))
        .with_context_provider(Arc::new(FailingContext))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("context failed"));
    let state = reduce_events(&event_store.load("run-1").await?)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn phase_failure_records_failed_replay_state() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = native_runtime()
        .with_event_store(event_store.clone())
        .insert_phase_after(PHASE_CONTEXT_PREPARE, Arc::new(FailingPhase))
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("phase failed"));
    let state = reduce_events(&event_store.load("run-1").await?)?;
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}
