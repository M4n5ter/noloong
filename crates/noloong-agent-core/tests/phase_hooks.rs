use noloong_agent_core::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, Agent, AgentCoreError, AgentEventKind, AgentRuntime,
    BeforeAssistantCommitHookContext, BeforeAssistantCommitHookResult,
    BeforeModelRequestHookContext, BeforeModelRequestHookResult, BoxFuture, CancellationToken,
    ContentBlock, EventStore, InMemoryEventStore, ModelProvider, ModelRequest, ModelStreamEvent,
    ModelStreamSink, PhaseHook, Result, RunStatus, StopReason, reduce_events,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

pub mod support;

use support::{
    assert_assistant_messages_contain, assert_assistant_text_contains, assistant_visible_text,
    assistant_visible_text_from_messages,
};

#[tokio::test]
async fn default_phase_hook_is_noop() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", false)))
        .with_phase_hook(Arc::new(NoopHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    assert_assistant_text_contains(&report, "raw");
    Ok(())
}

#[tokio::test]
async fn phase_hook_registration_preserves_order() -> Result<()> {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new_with_observer(
            "ok",
            false,
            Arc::clone(&observed),
        )))
        .with_phase_hook(Arc::new(RequestMetadataHook::new("first")))
        .with_phase_hook(Arc::new(RequestMetadataHook::new("second")))
        .max_turns(1)
        .build()?;

    runtime.run("hello").await?;

    let observed = observed.lock().await;
    let order = observed
        .first()
        .expect("model should receive a request")
        .metadata
        .get("order")
        .expect("metadata should contain hook order");
    assert_eq!(order, &json!(["first", "second"]));
    Ok(())
}

#[tokio::test]
async fn after_model_request_hook_rewrites_commit_input_without_double_recording() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", true)))
        .with_phase_hook(Arc::new(RewriteHook::after_model_request("rewritten")))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    assert_assistant_text_contains(&report, "rewritten");
    let raw_stream_events = report
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ModelStreamEvent {
                    event: ModelStreamEvent::TextDelta { text },
                    ..
                } if text == "raw"
            )
        })
        .count();
    let rewritten_stream_events = report
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ModelStreamEvent {
                    event: ModelStreamEvent::TextDelta { text },
                    ..
                } if text == "rewritten"
            )
        })
        .count();
    assert_eq!(raw_stream_events, 1);
    assert_eq!(rewritten_stream_events, 0);
    Ok(())
}

#[tokio::test]
async fn before_assistant_commit_hook_rewrites_events() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", false)))
        .with_phase_hook(Arc::new(RewriteHook::before_assistant_commit("before")))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    assert_assistant_text_contains(&report, "before");
    Ok(())
}

#[tokio::test]
async fn after_assistant_commit_hook_rewrites_message() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", false)))
        .with_phase_hook(Arc::new(RewriteHook::after_assistant_commit("after")))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    let visible_text = assistant_visible_text(&report);
    assert!(visible_text.contains("after"));
    assert!(!visible_text.contains("raw"));
    Ok(())
}

#[tokio::test]
async fn agent_builder_registers_phase_hooks() -> Result<()> {
    let agent = Agent::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", false)))
        .with_phase_hook(Arc::new(RewriteHook::after_assistant_commit("agent")))
        .build()?;

    agent.prompt("hello").await?;

    assert_assistant_messages_contain(&agent.state().await.messages, "agent");
    Ok(())
}

#[tokio::test]
async fn phase_hook_error_fails_without_assistant_commit() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let runtime = AgentRuntime::builder()
        .with_event_store(event_store.clone())
        .with_model_provider(Arc::new(StaticModel::new("raw", true)))
        .with_phase_hook(Arc::new(ErrorAfterModelRequestHook))
        .max_turns(1)
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;

    assert!(error.to_string().contains("phase hook failed"));
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(assistant_visible_text_from_messages(&state.messages).is_empty());
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseFailed { phase, error }
                if phase == "model.stream" && error.contains("phase hook failed")
        )
    }));
    Ok(())
}

#[tokio::test]
async fn cancelled_phase_hook_stops_later_hooks() -> Result<()> {
    let later_hook_called = Arc::new(AtomicBool::new(false));
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(StaticModel::new("raw", false)))
        .with_phase_hook(Arc::new(CancellingHook))
        .with_phase_hook(Arc::new(FlagHook::new(Arc::clone(&later_hook_called))))
        .max_turns(1)
        .build()?;

    let error = runtime.run("hello").await.unwrap_err();

    assert!(matches!(error, AgentCoreError::Aborted));
    assert!(!later_hook_called.load(Ordering::SeqCst));
    Ok(())
}

struct StaticModel {
    text: &'static str,
    emit_to_sink: bool,
    observed_requests: Option<Arc<Mutex<Vec<ModelRequest>>>>,
}

impl StaticModel {
    fn new(text: &'static str, emit_to_sink: bool) -> Self {
        Self {
            text,
            emit_to_sink,
            observed_requests: None,
        }
    }

    fn new_with_observer(
        text: &'static str,
        emit_to_sink: bool,
        observed_requests: Arc<Mutex<Vec<ModelRequest>>>,
    ) -> Self {
        Self {
            text,
            emit_to_sink,
            observed_requests: Some(observed_requests),
        }
    }
}

impl ModelProvider for StaticModel {
    fn id(&self) -> &str {
        "static"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            if let Some(observed_requests) = &self.observed_requests {
                observed_requests.lock().await.push(request);
            }
            let events = text_events(self.text);
            if self.emit_to_sink {
                for event in &events {
                    stream(event.clone()).await?;
                }
            }
            Ok(events)
        })
    }
}

struct NoopHook;

impl PhaseHook for NoopHook {}

struct RequestMetadataHook {
    label: &'static str,
}

impl RequestMetadataHook {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl PhaseHook for RequestMetadataHook {
    fn before_model_request<'a>(
        &'a self,
        context: BeforeModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let mut request = context.request.clone();
            let entry = request.metadata.entry("order").or_insert(json!([]));
            entry
                .as_array_mut()
                .expect("order metadata should be an array")
                .push(json!(self.label));
            Ok(Some(BeforeModelRequestHookResult { request }))
        })
    }
}

struct RewriteHook {
    target: RewriteTarget,
    text: &'static str,
}

impl RewriteHook {
    fn after_model_request(text: &'static str) -> Self {
        Self {
            target: RewriteTarget::AfterModelRequest,
            text,
        }
    }

    fn before_assistant_commit(text: &'static str) -> Self {
        Self {
            target: RewriteTarget::BeforeAssistantCommit,
            text,
        }
    }

    fn after_assistant_commit(text: &'static str) -> Self {
        Self {
            target: RewriteTarget::AfterAssistantCommit,
            text,
        }
    }
}

enum RewriteTarget {
    AfterModelRequest,
    BeforeAssistantCommit,
    AfterAssistantCommit,
}

impl PhaseHook for RewriteHook {
    fn after_model_request<'a>(
        &'a self,
        _context: AfterModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterModelRequestHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(
                matches!(self.target, RewriteTarget::AfterModelRequest).then(|| {
                    AfterModelRequestHookResult {
                        events: text_events(self.text),
                    }
                }),
            )
        })
    }

    fn before_assistant_commit<'a>(
        &'a self,
        _context: BeforeAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeAssistantCommitHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(
                matches!(self.target, RewriteTarget::BeforeAssistantCommit).then(|| {
                    BeforeAssistantCommitHookResult {
                        events: text_events(self.text),
                    }
                }),
            )
        })
    }

    fn after_assistant_commit<'a>(
        &'a self,
        context: AfterAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterAssistantCommitHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            if !matches!(self.target, RewriteTarget::AfterAssistantCommit) {
                return Ok(None);
            }
            let mut message = context.message.clone();
            message.content = vec![ContentBlock::Text {
                text: self.text.to_string(),
            }];
            Ok(Some(AfterAssistantCommitHookResult { message }))
        })
    }
}

struct ErrorAfterModelRequestHook;

impl PhaseHook for ErrorAfterModelRequestHook {
    fn after_model_request<'a>(
        &'a self,
        _context: AfterModelRequestHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterModelRequestHookResult>> {
        Box::pin(async { Err(AgentCoreError::Phase("phase hook failed".into())) })
    }
}

struct CancellingHook;

impl PhaseHook for CancellingHook {
    fn before_model_request<'a>(
        &'a self,
        _context: BeforeModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async move {
            cancellation.cancel();
            Ok(None)
        })
    }
}

struct FlagHook {
    called: Arc<AtomicBool>,
}

impl FlagHook {
    fn new(called: Arc<AtomicBool>) -> Self {
        Self { called }
    }
}

impl PhaseHook for FlagHook {
    fn before_model_request<'a>(
        &'a self,
        _context: BeforeModelRequestHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async move {
            self.called.store(true, Ordering::SeqCst);
            Ok(None)
        })
    }
}

fn text_events(text: impl Into<String>) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::Started {
            stream_id: "test-stream".into(),
        },
        ModelStreamEvent::TextDelta { text: text.into() },
        ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop,
        },
    ]
}
