use noloong_agent_core::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AgentEventKind, AgentRuntime,
    AgentRuntimeBuilder, BoxFuture, CancellationToken, ContentBlock, ContextCompactionConfig,
    ContextPatch, EventStore, HttpAuthContext, HttpAuthProvider, HttpAuthRefreshContext,
    InMemoryEventStore, MediaEncoding, MediaKind, MediaSource, MessageRole, ModelProvider,
    ModelStreamEvent, PhaseHook, ResponsesApiProvider, ResponsesApiProviderConfig, Result,
    RunStatus, StdioExtension, StdioExtensionConfig, StdioHttpAuthProvider, ToolApprovalResolution,
    ToolPermissionDecision, ToolPermissionOutcome, reduce_events,
};
use serde_json::{Map, json};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::timeout};

pub mod support;

use support::{
    MockResponse, MockServer, assert_assistant_text_contains, compaction_trigger_state,
    fixture_path,
};

#[tokio::test]
async fn stdio_compaction_summarizer_compacts_persistent_state() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_context_compaction_summarizer_id(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10),
            "fixture-compaction",
        )
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--compaction-summarizer-mode=summary".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await?;

    assert!(report.state.messages.iter().any(|message| {
        matches!(message.role, MessageRole::System)
            && message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text }
                        if text.contains("fixture compaction summary")
                )
            })
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::CompactMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn malformed_stdio_compaction_summarizer_response_fails_phase() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_context_compaction_summarizer_id(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10),
            "fixture-compaction",
        )
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--compaction-summarizer-mode=malformed".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let result = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await;

    let error = result.expect_err("malformed compaction response should fail");
    assert!(
        error.to_string().contains("json"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_stdio_context_compactor_response_fails_phase() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_context_compactor_id(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10),
            "fixture-context-compactor",
        )
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--context-compactor-mode=malformed".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let result = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await;

    let error = result.expect_err("malformed context compactor response should fail");
    assert!(
        error.to_string().contains("json"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[tokio::test]
async fn stdio_context_compactor_replaces_persistent_state() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_context_compactor_id(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10),
            "fixture-context-compactor",
        )
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--context-compactor-mode=replacement".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.contains("fixture replacement summary")
            )
        })
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::ReplaceMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn stdio_context_compactor_can_return_summary() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_context_compactor_id(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10),
            "fixture-context-compactor",
        )
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--context-compactor-mode=summary".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime
        .continue_from_state(compaction_trigger_state(), None, CancellationToken::new())
        .await?;

    assert!(report.state.messages.iter().any(|message| {
        matches!(message.role, MessageRole::System)
            && message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text }
                        if text.contains("fixture context compactor summary")
                )
            })
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::CompactMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn malformed_stdio_http_auth_provider_response_fails_request() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let extension = Arc::new(
        StdioExtension::connect(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--auth-provider-mode=malformed".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?,
    );
    let auth = StdioHttpAuthProvider::new(extension, "fixture-auth".into());

    let error = auth
        .headers(
            HttpAuthContext::new(
                "test-responses",
                "POST",
                "https://example.test/responses",
                0,
            ),
            CancellationToken::new(),
        )
        .await
        .expect_err("malformed auth headers should fail");

    assert!(error.to_string().contains("json"));
    Ok(())
}

#[tokio::test]
async fn stdio_http_auth_provider_can_deny_refresh() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let extension = Arc::new(
        StdioExtension::connect(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--auth-provider-mode=deny-refresh".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?,
    );
    let auth = StdioHttpAuthProvider::new(extension, "fixture-auth".into());

    let result = auth
        .refresh(
            HttpAuthRefreshContext::unauthorized(
                HttpAuthContext::new(
                    "test-responses",
                    "POST",
                    "https://example.test/responses",
                    0,
                ),
                401,
            ),
            CancellationToken::new(),
        )
        .await?;

    assert!(!result.retry);
    Ok(())
}

#[tokio::test]
async fn stdio_http_auth_provider_supplies_headers() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let extension = Arc::new(
        StdioExtension::connect(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--auth-provider-mode=headers".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?,
    );
    let auth = Arc::new(StdioHttpAuthProvider::new(extension, "fixture-auth".into()));
    let server = MockServer::spawn(200, "text/event-stream", "data: [DONE]\n\n").await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .api_key("static-secret")
            .auth_provider(auth),
    )?;

    provider
        .stream_model(
            noloong_agent_core::ModelRequest {
                run_id: "run-1".into(),
                turn_id: 1,
                messages: vec![noloong_agent_core::AgentMessage::user("user-1", "hello")],
                context: Map::new(),
                tools: Vec::new(),
                metadata: Map::new(),
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let request = server.request();
    assert_eq!(request.header("authorization"), Some("Bearer fixture-auth"));
    assert_eq!(request.header("x-fixture-auth"), Some("test-responses"));
    Ok(())
}

#[tokio::test]
async fn stdio_http_auth_provider_refreshes_after_unauthorized() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let extension = Arc::new(
        StdioExtension::connect(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--auth-provider-mode=headers".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?,
    );
    let auth = Arc::new(StdioHttpAuthProvider::new(extension, "fixture-auth".into()));
    let server = MockServer::spawn_many(vec![
        MockResponse::new(401, "application/json", "{\"error\":\"expired\"}"),
        MockResponse::new(200, "text/event-stream", "data: [DONE]\n\n"),
    ])
    .await?;
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("test-responses", "test-model")
            .base_url(server.url())
            .auth_provider(auth),
    )?;

    provider
        .stream_model(
            noloong_agent_core::ModelRequest {
                run_id: "run-1".into(),
                turn_id: 1,
                messages: vec![noloong_agent_core::AgentMessage::user("user-1", "hello")],
                context: Map::new(),
                tools: Vec::new(),
                metadata: Map::new(),
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
            CancellationToken::new(),
        )
        .await?;

    let requests = server.requests();
    assert_eq!(
        requests[0].header("authorization"),
        Some("Bearer fixture-auth")
    );
    assert_eq!(
        requests[1].header("authorization"),
        Some("Bearer fixture-refresh")
    );
    Ok(())
}

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
async fn jsonrpc_model_stream_media_delta() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--media-stream".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime.run("media").await?;

    let assistant = report
        .state
        .messages
        .iter()
        .find(|message| matches!(message.role, noloong_agent_core::MessageRole::Assistant))
        .expect("assistant message should be committed");
    assert!(assistant.content.iter().any(|block| {
        matches!(
            block,
            ContentBlock::Media {
                media:
                    noloong_agent_core::MediaBlock {
                        kind: MediaKind::Image,
                        source:
                            MediaSource::Inline {
                                data,
                                encoding: MediaEncoding::Base64,
                            },
                        ..
                    },
            } if data == "aW1hZ2U="
        )
    }));
    Ok(())
}

#[tokio::test]
async fn jsonrpc_tool_output_media() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([fixture.to_string_lossy().to_string(), "--media-tool".into()])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(1).build()?;

    let report = runtime.run("tool media").await?;

    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { content, .. }
                    if matches!(
                        content.first(),
                        Some(ContentBlock::Media {
                            media:
                                noloong_agent_core::MediaBlock {
                                    kind: MediaKind::File,
                                    source:
                                        MediaSource::Provider {
                                            provider_id,
                                            id,
                                        },
                                    ..
                                },
                        }) if provider_id == "fixture-model" && id == "fixture-file-1"
                    )
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn stdio_phase_hook_before_model_request_modifies_request() -> Result<()> {
    let runtime = runtime_with_phase_hook_mode("before-request").await?;

    let report = runtime.run("hook").await?;

    assert_assistant_text_contains(&report, "hooked request");
    Ok(())
}

#[tokio::test]
async fn stdio_phase_hook_after_model_request_modifies_events() -> Result<()> {
    let runtime = runtime_with_phase_hook_mode("after-events").await?;

    let report = runtime.run("hook").await?;

    assert_assistant_text_contains(&report, "hooked events");
    Ok(())
}

#[tokio::test]
async fn stdio_phase_hook_after_assistant_commit_modifies_message() -> Result<()> {
    let runtime = runtime_with_phase_hook_mode("after-assistant").await?;

    let report = runtime.run("hook").await?;

    assert_assistant_text_contains(&report, "hooked assistant");
    Ok(())
}

#[tokio::test]
async fn stdio_phase_hook_composes_with_native_hooks_in_registration_order() -> Result<()> {
    let builder = phase_hook_builder_from(
        AgentRuntime::builder().with_phase_hook(Arc::new(AppendAssistantTextHook::new(" before"))),
        "after-assistant",
    )
    .await?
    .with_phase_hook(Arc::new(AppendAssistantTextHook::new(" after")));
    let runtime = builder.max_turns(1).build()?;

    let report = runtime.run("hook").await?;

    assert_assistant_text_contains(&report, "hooked assistant after");
    Ok(())
}

#[tokio::test]
async fn malformed_stdio_phase_hook_response_fails_active_phase() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = phase_hook_builder_from(
        AgentRuntime::builder().with_event_store(event_store.clone()),
        "malformed",
    )
    .await?;
    let runtime = builder.max_turns(1).build()?;

    let error = runtime.run("hook").await.unwrap_err();
    let events = event_store.load("run-1").await?;

    assert!(error.to_string().contains("invalid type"));
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseFailed { phase, .. } if phase == "model.request.prepare"
        )
    }));
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
async fn stdio_tool_call_hook_can_allow_and_audit_tool_execution() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--tool-hook-mode=allow".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(4).build()?;

    let report = runtime.run("hello").await?;

    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                hook_id,
                decision,
                ..
            } if hook_id.as_deref() == Some("fixture-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Allow
                && decision.approver.as_deref() == Some("stdio-fixture")
        )
    }));
    assert_assistant_text_contains(&report, "done from fixture");
    Ok(())
}

#[tokio::test]
async fn stdio_tool_call_hook_can_pause_for_approval_and_resume() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = AgentRuntime::builder()
        .with_event_store(event_store)
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--tool-hook-mode=approval".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(4).build()?;

    let paused = runtime.run("hello").await?;

    assert!(matches!(paused.state.status, RunStatus::Paused));
    assert_eq!(paused.state.pending_tool_approvals.len(), 1);
    let approval_id = paused
        .state
        .pending_tool_approvals
        .keys()
        .next()
        .expect("approval should be pending")
        .clone();
    assert!(paused.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolApprovalRequested { approval }
            if approval.request.prompt.as_deref() == Some("Approve fixture tool?")
                && approval.hook_id.as_deref() == Some("fixture-tool-hook"))
    }));

    let resumed = runtime
        .resume_tool_approvals(
            &paused.run_id,
            vec![ToolApprovalResolution {
                approval_id,
                decision: ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("approved by jsonrpc test".into()),
                    approver: Some("jsonrpc-test".into()),
                    metadata: json!({}),
                },
            }],
            None,
            CancellationToken::new(),
        )
        .await?;

    assert!(matches!(resumed.state.status, RunStatus::Completed));
    assert_assistant_text_contains(&resumed, "done from fixture");
    assert!(resumed.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                hook_id,
                decision,
                ..
            } if hook_id.as_deref() == Some("fixture-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Allow
                && decision.approver.as_deref() == Some("jsonrpc-test")
        )
    }));
    Ok(())
}

#[tokio::test]
async fn stdio_tool_call_hook_can_deny_tool_execution() -> Result<()> {
    let fixture = fixture_path("stdio-extension.mjs");
    let builder = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--tool-hook-mode=deny".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?;
    let runtime = builder.max_turns(4).build()?;

    let report = runtime.run("hello").await?;

    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    content,
                    is_error,
                    ..
                } if tool_name == "fixture_echo"
                    && *is_error
                    && content.iter().any(|block| {
                        matches!(
                            block,
                            ContentBlock::Text { text }
                                if text.contains("denied by fixture tool hook")
                        )
                    })
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn malformed_stdio_tool_call_hook_response_fails_active_phase() -> Result<()> {
    let event_store = Arc::new(InMemoryEventStore::new());
    let builder = AgentRuntime::builder().with_event_store(event_store.clone());
    let fixture = fixture_path("stdio-extension.mjs");
    let runtime = builder
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    "--tool-hook-mode=malformed".into(),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await?
        .max_turns(2)
        .build()?;

    let error = runtime.run("hello").await.expect_err("run should fail");

    assert!(
        error.to_string().contains("json"),
        "unexpected error: {error}"
    );
    let events = event_store.load("run-1").await?;
    let state = reduce_events(&events)?;
    assert!(matches!(state.status, RunStatus::Failed));
    assert!(events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseFailed { phase, .. } if phase == "tool.execute"
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

async fn runtime_with_phase_hook_mode(mode: &str) -> Result<AgentRuntime> {
    phase_hook_builder_from(AgentRuntime::builder(), mode)
        .await?
        .max_turns(1)
        .build()
}

async fn phase_hook_builder_from(
    builder: AgentRuntimeBuilder,
    mode: &str,
) -> Result<AgentRuntimeBuilder> {
    let fixture = fixture_path("stdio-extension.mjs");
    builder
        .with_stdio_extension(
            StdioExtensionConfig::new("node")
                .args([
                    fixture.to_string_lossy().to_string(),
                    format!("--phase-hook-mode={mode}"),
                ])
                .request_timeout(Duration::from_secs(2)),
        )
        .await
}

struct AppendAssistantTextHook {
    suffix: &'static str,
}

impl AppendAssistantTextHook {
    fn new(suffix: &'static str) -> Self {
        Self { suffix }
    }
}

impl PhaseHook for AppendAssistantTextHook {
    fn after_assistant_commit<'a>(
        &'a self,
        context: AfterAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterAssistantCommitHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let mut message = context.message.clone();
            if let Some(ContentBlock::Text { text }) = message.content.first_mut() {
                text.push_str(self.suffix);
            }
            Ok(Some(AfterAssistantCommitHookResult { message }))
        })
    }
}
