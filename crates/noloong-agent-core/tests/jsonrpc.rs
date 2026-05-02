use noloong_agent_core::{
    AgentEventKind, AgentRuntime, ContextPatch, EventStore, InMemoryEventStore, ModelStreamEvent,
    Result, RunStatus, StdioExtension, StdioExtensionConfig, reduce_events,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};

#[tokio::test]
async fn stdio_extension_runs_provider_tool_and_context() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .arg(fixture.to_string_lossy())
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(4).build()?;

    let report = runtime.run("hello").await?;

    assert_eq!(report.state.context.get("fixture"), Some(&json!("context")));
    assert_eq!(
        report.state.context.get("fixture_phase"),
        Some(&json!(true))
    );
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            serde_json::to_value(block)
                .expect("content block serializes")
                .to_string()
                .contains("done from fixture")
        })
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::PatchContext {
                    patch: ContextPatch::Set { key, .. }
                }
            } if key == "fixture"
        )
    }));
    assert!(
        report
            .events
            .iter()
            .any(|event| { matches!(&event.kind, AgentEventKind::ModelStreamEvent { .. }) })
    );
    assert!(
        report
            .events
            .iter()
            .any(|event| { matches!(&event.kind, AgentEventKind::ToolExecutionCompleted { .. }) })
    );
    Ok(())
}

#[tokio::test]
async fn stdio_extension_supports_lifecycle_methods() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let extension = StdioExtension::connect(
        StdioExtensionConfig::new("node")
            .arg(fixture.to_string_lossy())
            .request_timeout(Duration::from_secs(2)),
    )
    .await?;

    assert_eq!(extension.manifest().name, "stdio-fixture");
    let capabilities = extension.capabilities().await?;
    assert!(!capabilities.is_empty());
    extension.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn stdio_model_stream_notifications_are_incremental() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--delayed-stream".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;
    let (sender, mut receiver) = mpsc::channel(4);

    let run = tokio::spawn(async move {
        runtime
            .run_with_events("hello", move |event| {
                let sender = sender.clone();
                async move {
                    if let AgentEventKind::ModelStreamEvent {
                        event: ModelStreamEvent::TextDelta { text },
                        ..
                    } = event.kind
                    {
                        let _ = sender.send(text).await;
                    }
                    Ok(())
                }
            })
            .await
    });

    let text = timeout(Duration::from_millis(75), receiver.recv())
        .await
        .expect("stream delta should arrive before JSON-RPC response delay")
        .expect("stream delta should be sent");
    assert_eq!(text, "delayed chunk");
    run.await.expect("runtime task joins")?;
    Ok(())
}

#[tokio::test]
async fn stdio_model_stream_can_finish_before_jsonrpc_response() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--stream-no-response".into(),
                ])
                .request_timeout(Duration::from_secs(5))
                .stream_timeout(Duration::from_millis(500)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = timeout(Duration::from_millis(200), runtime.run("hello"))
        .await
        .expect("stream terminal event should settle the run before request timeout")?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, noloong_agent_core::ContentBlock::Text { text } if text == "terminal chunk"),
        )
    }));
    Ok(())
}

#[tokio::test]
async fn stdio_model_stream_timeout_is_separate_from_request_timeout() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--stream-hangs".into(),
                ])
                .request_timeout(Duration::from_secs(5))
                .stream_timeout(Duration::from_millis(75)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let error = timeout(Duration::from_millis(500), runtime.run("hello"))
        .await
        .expect("stream timeout should fire before request timeout")
        .unwrap_err();

    assert!(error.to_string().contains("model stream timed out"));
    Ok(())
}

#[tokio::test]
async fn stdio_model_stream_error_records_failed_replay_state() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--stream-error".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let error = runtime.run("hello").await.unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(error.to_string().contains("model stream failed"));
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
async fn stdio_extension_crash_records_failed_replay_state() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--crash-on-model".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let error = runtime.run("hello").await.unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(error.to_string().contains("extension stdout closed"));
    assert!(matches!(state.status, RunStatus::Failed));
    Ok(())
}

#[tokio::test]
async fn invalid_json_from_stdio_extension_is_reported() {
    let fixture = fixture_path("stdio-extension.mjs");
    let result = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--invalid-json".into(),
                ])
                .request_timeout(Duration::from_millis(500)),
        )
        .await;

    let error = match result {
        Ok(_) => panic!("invalid JSON extension unexpectedly connected"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("invalid json from extension"));
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
