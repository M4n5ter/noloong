use noloong_agent_core::{
    AgentCoreError, AgentEventKind, AgentInput, AgentRuntime, AgentRuntimeBuilder,
    CancellationToken, ContentBlock, ContextCompactionConfig, EventSinkFuture, EventStore,
    ExtensionConformanceCaseStatus, ExtensionConformanceConfig, ExtensionConformanceProfile,
    InMemoryEventStore, ModelStreamEvent, Result, RunReport, RunStatus, StdioExtension,
    ToolPermissionOutcome, reduce_events, run_extension_conformance,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};

pub mod support;

use support::{
    assert_assistant_text_contains, compaction_trigger_state, jsonrpc_conformance_config as config,
    jsonrpc_conformance_config_with_timeouts as config_with_timeouts,
};

const MODE_ALL_CAPABILITIES: &str = "all-capabilities";
const MODE_ADAPTER_PAYLOADS: &str = "adapter-payloads";
const MODE_DELAYED_STREAM: &str = "delayed-stream";
const MODE_DUPLICATE_COMPACTION: &str = "duplicate-compaction";
const MODE_DUPLICATE_CONTEXT: &str = "duplicate-context";
const MODE_DUPLICATE_MODEL: &str = "duplicate-model";
const MODE_DUPLICATE_PHASE: &str = "duplicate-phase";
const MODE_DUPLICATE_PHASE_HOOK: &str = "duplicate-phase-hook";
const MODE_DUPLICATE_TOOL_CALL_HOOK: &str = "duplicate-tool-call-hook";
const MODE_DUPLICATE_TOOL: &str = "duplicate-tool";
const MODE_INVALID_STREAM_RESULT: &str = "invalid-stream-result";
const MODE_LATE_RESPONSE_AFTER_CANCEL: &str = "late-response-after-cancel";
const MODE_MALFORMED_ACTIVE_STREAM: &str = "malformed-active-stream";
const MODE_MALFORMED_CAPABILITIES: &str = "malformed-capabilities";
const MODE_MALFORMED_COMPACTION_RESULT: &str = "malformed-compaction-result";
const MODE_MALFORMED_CONTEXT_RESULT: &str = "malformed-context-result";
const MODE_MALFORMED_MANIFEST: &str = "malformed-manifest";
const MODE_MALFORMED_PHASE_HOOK_RESULT: &str = "malformed-phase-hook-result";
const MODE_MALFORMED_PHASE_RESULT: &str = "malformed-phase-result";
const MODE_MALFORMED_TOOL_HOOK_RESULT: &str = "malformed-tool-hook-result";
const MODE_MALFORMED_TOOL_RESULT: &str = "malformed-tool-result";
const MODE_MISSING_RESULT: &str = "missing-result";
const MODE_MODEL_JSONRPC_ERROR: &str = "model-jsonrpc-error";
const MODE_RESPONSE_BUFFERED_EVENTS: &str = "response-buffered-events";
const MODE_STDOUT_CLOSE: &str = "stdout-close";
const MODE_STREAM_HANGS: &str = "stream-hangs";
const MODE_STREAM_NO_RESPONSE: &str = "stream-no-response";
const MODE_TOOL_HOOK_DENY: &str = "tool-hook-deny";
const MODE_TOOL_HOOK_PAYLOADS: &str = "tool-hook-payloads";
const MODE_UNKNOWN_CAPABILITY: &str = "unknown-capability";
const MODE_UNKNOWN_STREAM_NOTIFICATION: &str = "unknown-stream-notification";
const MODE_WRONG_RESPONSE_ID: &str = "wrong-response-id";

#[tokio::test]
async fn lifecycle_malformed_manifest_fails_connect() {
    let error = connect_error(&[MODE_MALFORMED_MANIFEST]).await;

    assert_contains(&error, "json");
}

#[tokio::test]
async fn public_runner_strict_fixture_passes() -> Result<()> {
    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(config(&[
            MODE_ALL_CAPABILITIES,
            MODE_ADAPTER_PAYLOADS,
            MODE_TOOL_HOOK_PAYLOADS,
        ]))
        .profile(ExtensionConformanceProfile::Strict),
    )
    .await?;

    assert!(
        report.is_success(),
        "strict conformance report failed: {report:?}"
    );
    assert_eq!(report.failed(), 0);
    assert_eq!(report.skipped(), 0);
    assert!(report.cases.iter().any(|case| {
        case.name == "adapter_payloads" && case.status == ExtensionConformanceCaseStatus::Passed
    }));
    assert!(report.cases.iter().any(|case| {
        case.name == "compaction_summarizer"
            && case.status == ExtensionConformanceCaseStatus::Passed
    }));
    Ok(())
}

#[tokio::test]
async fn capabilities_malformed_or_unknown_fail_registration() {
    for mode in [MODE_MALFORMED_CAPABILITIES, MODE_UNKNOWN_CAPABILITY] {
        let error = builder_error(&[mode]).await;
        assert_contains(&error, "json");
    }
}

#[tokio::test]
async fn capabilities_duplicate_ids_fail_registration() {
    for mode in [
        MODE_DUPLICATE_MODEL,
        MODE_DUPLICATE_TOOL,
        MODE_DUPLICATE_CONTEXT,
        MODE_DUPLICATE_PHASE,
        MODE_DUPLICATE_PHASE_HOOK,
        MODE_DUPLICATE_TOOL_CALL_HOOK,
        MODE_DUPLICATE_COMPACTION,
    ] {
        let error = builder_error(&[mode]).await;
        assert_contains(&error, "duplicate");
    }
}

#[tokio::test]
async fn request_response_jsonrpc_error_is_reported() -> Result<()> {
    let error = run_error(&[MODE_MODEL_JSONRPC_ERROR]).await?;

    assert_contains(&error, "json-rpc error");
    assert_contains(&error, "fixture model jsonrpc error");
    Ok(())
}

#[tokio::test]
async fn request_response_wrong_id_times_out() -> Result<()> {
    let error = runtime_with_timeouts(
        &[MODE_WRONG_RESPONSE_ID],
        Duration::from_millis(500),
        Duration::from_secs(5),
    )
    .await?
    .run("hello")
    .await
    .expect_err("wrong response id should leave request pending")
    .to_string();

    assert_contains(&error, "request timed out: model/stream");
    Ok(())
}

#[tokio::test]
async fn request_response_missing_or_invalid_result_fails() -> Result<()> {
    for mode in [MODE_MISSING_RESULT, MODE_INVALID_STREAM_RESULT] {
        let error = run_error(&[mode]).await?;
        assert_contains(&error, "json");
    }
    Ok(())
}

#[tokio::test]
async fn request_response_stdout_close_fails_pending_request() -> Result<()> {
    let error = run_error(&[MODE_STDOUT_CLOSE]).await?;

    assert_contains(&error, "extension stdout closed");
    Ok(())
}

#[tokio::test]
async fn request_response_cancellation_removes_pending_request() -> Result<()> {
    let runtime = Arc::new(runtime(&[MODE_LATE_RESPONSE_AFTER_CANCEL]).await?);
    let cancellation = CancellationToken::new();
    let run_cancellation = cancellation.clone();
    let run_runtime = Arc::clone(&runtime);

    let handle = tokio::spawn(async move {
        run_runtime
            .run_with_event_sink(
                AgentInput::from("hello"),
                Arc::new(|_event| Box::pin(async { Ok(()) }) as EventSinkFuture),
                run_cancellation,
            )
            .await
    });

    sleep(Duration::from_millis(25)).await;
    cancellation.cancel();
    let error = handle
        .await?
        .expect_err("run should abort after cancellation");
    assert!(matches!(error, AgentCoreError::Aborted));
    sleep(Duration::from_millis(200)).await;
    Ok(())
}

#[tokio::test]
async fn malformed_model_tool_context_phase_and_hook_results_fail_active_phase() -> Result<()> {
    for (mode, phase, snippet) in [
        (MODE_MALFORMED_CONTEXT_RESULT, "context.prepare", "json"),
        (MODE_MALFORMED_PHASE_RESULT, "conformance.phase", "json"),
        (
            MODE_MALFORMED_PHASE_HOOK_RESULT,
            "model.request.prepare",
            "json",
        ),
        (MODE_MALFORMED_TOOL_HOOK_RESULT, "tool.execute", "json"),
    ] {
        let modes = if mode == MODE_MALFORMED_TOOL_HOOK_RESULT {
            vec![MODE_ALL_CAPABILITIES, MODE_ADAPTER_PAYLOADS, mode]
        } else {
            vec![MODE_ALL_CAPABILITIES, mode]
        };
        assert_failed_phase(&modes, phase, snippet).await?;
    }
    Ok(())
}

#[tokio::test]
async fn jsonrpc_tool_hook_denies_tool_call_with_audit() -> Result<()> {
    let report = runtime(&[
        MODE_ALL_CAPABILITIES,
        MODE_ADAPTER_PAYLOADS,
        MODE_TOOL_HOOK_DENY,
    ])
    .await?
    .run("hello")
    .await?;

    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                tool_call_id,
                decision,
                ..
            } if tool_call_id == "conformance-call-1"
                && decision.outcome == ToolPermissionOutcome::Deny
                && decision.metadata.get("fixture").and_then(serde_json::Value::as_str)
                    == Some("tool-hook-deny")
        )
    }));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    content,
                    is_error,
                    ..
                } if tool_name == "conformance_echo"
                    && *is_error
                    && content.iter().any(|block| {
                        matches!(
                            block,
                            ContentBlock::Text { text }
                                if text.contains("denied by conformance tool hook")
                        )
                    })
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn malformed_tool_result_becomes_auditable_error_output() -> Result<()> {
    let report = runtime(&[
        MODE_ALL_CAPABILITIES,
        MODE_ADAPTER_PAYLOADS,
        MODE_MALFORMED_TOOL_RESULT,
    ])
    .await?
    .run("hello")
    .await?;

    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { output, .. }
                if output.is_error
                    && output.content.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text.contains("json error"))
                    })
        )
    }));
    Ok(())
}

#[tokio::test]
async fn malformed_compaction_result_fails_context_compact_phase() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = builder_with_store(
        &[MODE_ALL_CAPABILITIES, MODE_MALFORMED_COMPACTION_RESULT],
        Arc::clone(&event_store),
    )
    .await?
    .with_context_compaction_summarizer_id(
        ContextCompactionConfig::new(64)
            .reserve_tokens(8)
            .keep_recent_tokens(10),
        "conformance-compaction",
    );
    let runtime = builder.max_turns(1).build()?;

    let error = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await
        .expect_err("malformed compaction should fail");

    assert_contains(&error.to_string(), "json");
    assert_event_store_failed_phase(event_store, "context.compact").await?;
    Ok(())
}

#[tokio::test]
async fn stream_event_arrives_before_response() -> Result<()> {
    let runtime = runtime(&[MODE_DELAYED_STREAM]).await?;
    let (sender, mut receiver) = mpsc::channel(1);

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
        .expect("stream event should arrive before delayed response")
        .expect("stream event should be sent");
    run.await??;

    assert_eq!(text, "delayed chunk");
    Ok(())
}

#[tokio::test]
async fn stream_response_buffered_events_are_not_duplicated() -> Result<()> {
    let report = runtime(&[MODE_RESPONSE_BUFFERED_EVENTS])
        .await?
        .run("hello")
        .await?;
    let count = model_text_delta_count(&report, "buffered response");

    assert_eq!(count, 1);
    assert_assistant_text_contains(&report, "buffered response");
    Ok(())
}

#[tokio::test]
async fn stream_terminal_event_settles_without_response() -> Result<()> {
    let runtime = runtime(&[MODE_STREAM_NO_RESPONSE]).await?;

    let report = timeout(Duration::from_millis(250), runtime.run("hello"))
        .await
        .expect("terminal stream event should settle without response")?;

    assert_assistant_text_contains(&report, "terminal chunk");
    Ok(())
}

#[tokio::test]
async fn stream_malformed_active_event_fails_immediately() -> Result<()> {
    let runtime = runtime_with_timeouts(
        &[MODE_MALFORMED_ACTIVE_STREAM],
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .await?;

    let error = timeout(Duration::from_millis(500), runtime.run("hello"))
        .await
        .expect("malformed active stream event should fail before timeout")
        .expect_err("run should fail");

    assert_contains(&error.to_string(), "invalid stream event");
    Ok(())
}

#[tokio::test]
async fn stream_unknown_stream_notification_does_not_affect_active_stream() -> Result<()> {
    let report = runtime(&[MODE_UNKNOWN_STREAM_NOTIFICATION])
        .await?
        .run("hello")
        .await?;

    assert_assistant_text_contains(&report, "unknown stream ok");
    assert_eq!(model_text_delta_count(&report, "ignored"), 0);
    Ok(())
}

#[tokio::test]
async fn stream_timeout_is_independent_from_request_timeout() -> Result<()> {
    let runtime = runtime_with_timeouts(
        &[MODE_STREAM_HANGS],
        Duration::from_secs(5),
        Duration::from_millis(75),
    )
    .await?;

    let error = timeout(Duration::from_millis(500), runtime.run("hello"))
        .await
        .expect("stream timeout should fire before request timeout")
        .expect_err("run should fail");

    assert_contains(&error.to_string(), "model stream timed out");
    Ok(())
}

async fn builder(modes: &[&str]) -> Result<AgentRuntimeBuilder> {
    AgentRuntime::builder()
        .with_stdio_extension(config(modes))
        .await
}

async fn builder_with_store(
    modes: &[&str],
    event_store: Arc<InMemoryEventStore>,
) -> Result<AgentRuntimeBuilder> {
    AgentRuntime::builder()
        .with_event_store(event_store)
        .with_stdio_extension(config(modes))
        .await
}

async fn runtime(modes: &[&str]) -> Result<AgentRuntime> {
    builder(modes).await?.max_turns(2).build()
}

async fn runtime_with_timeouts(
    modes: &[&str],
    request_timeout: Duration,
    stream_timeout: Duration,
) -> Result<AgentRuntime> {
    AgentRuntime::builder()
        .with_stdio_extension(config_with_timeouts(modes, request_timeout, stream_timeout))
        .await?
        .max_turns(2)
        .build()
}

async fn connect_error(modes: &[&str]) -> String {
    match StdioExtension::connect(config(modes)).await {
        Ok(_) => panic!("extension connection should fail"),
        Err(error) => error.to_string(),
    }
}

async fn builder_error(modes: &[&str]) -> String {
    match AgentRuntime::builder()
        .with_stdio_extension(config(modes))
        .await
    {
        Ok(_) => panic!("builder registration should fail"),
        Err(error) => error.to_string(),
    }
}

async fn run_error(modes: &[&str]) -> Result<String> {
    Ok(runtime(modes)
        .await?
        .run("hello")
        .await
        .expect_err("run should fail")
        .to_string())
}

async fn assert_failed_phase(
    modes: &[&str],
    expected_phase: &str,
    error_snippet: &str,
) -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = builder_with_store(modes, Arc::clone(&event_store))
        .await?
        .max_turns(2)
        .build()?;

    let error = runtime.run("hello").await.expect_err("run should fail");

    assert_contains(&error.to_string(), error_snippet);
    assert_event_store_failed_phase(event_store, expected_phase).await
}

async fn assert_event_store_failed_phase(
    event_store: Arc<InMemoryEventStore>,
    expected_phase: &str,
) -> Result<()> {
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseFailed { phase, .. } if phase == expected_phase
        )
    }));
    Ok(())
}

fn assert_contains(value: &str, expected: &str) {
    assert!(
        value.contains(expected),
        "expected `{expected}` in `{value}`"
    );
}

fn model_text_delta_count(report: &RunReport, expected: &str) -> usize {
    report
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ModelStreamEvent {
                    event: ModelStreamEvent::TextDelta { text },
                    ..
                } if text == expected
            )
        })
        .count()
}
