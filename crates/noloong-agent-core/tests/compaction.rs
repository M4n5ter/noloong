use noloong_agent_core::{
    AgentCoreError, AgentEffect, AgentEventKind, AgentMessage, AgentRuntime, AgentState, BoxFuture,
    CancellationToken, CompactionDecision, CompactionSummarizer, CompactionSummaryRequest,
    CompactionSummaryResult, ContentBlock, ContextCompactionConfig, ContextCompactionMode,
    ContextCompactionOutput, ContextCompactionRequest, ContextCompactor, HeuristicTokenEstimator,
    MediaBlock, MediaKind, MessageCompaction, MessageReplacement, MessageRole,
    ModelBackedCompactionSummarizer, ModelBackedCompactionSummarizerConfig, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, PHASE_CONTEXT_COMPACT, Result, StopReason,
    TokenEstimator, ToolCall, ToolOutput, compacted_messages, compaction_summary_message,
    plan_compaction, reduce_events,
};
use serde_json::{Map, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn compaction_config_and_summary_request_serde_round_trip() -> Result<()> {
    let config = ContextCompactionConfig::new(64_000)
        .reserve_tokens(4_000)
        .keep_recent_tokens(8_000)
        .mode(ContextCompactionMode::RequestOnly)
        .metadata("owner", json!("test"));
    config.validate()?;

    let encoded = serde_json::to_value(&config)?;
    assert_eq!(encoded["contextWindowTokens"], 64_000);
    assert_eq!(encoded["mode"], "request_only");
    assert_eq!(
        serde_json::from_value::<ContextCompactionConfig>(encoded)?,
        config
    );

    let request = CompactionSummaryRequest {
        run_id: "run-1".into(),
        turn_id: 2,
        previous_summary: Some("old summary".into()),
        messages_to_summarize: vec![message("u1", MessageRole::User, "hello")],
        turn_prefix_messages: Vec::new(),
        token_budget: 512,
        metadata: Map::new(),
    };
    let value = serde_json::to_value(&request)?;
    assert_eq!(
        serde_json::from_value::<CompactionSummaryRequest>(value)?,
        request
    );

    assert!(
        ContextCompactionConfig::new(100)
            .reserve_tokens(100)
            .validate()
            .is_err()
    );
    Ok(())
}

#[test]
fn compact_messages_effect_replays_to_compacted_state() -> Result<()> {
    let summary = compaction_summary_message("run-1", 1, "summary".into(), Map::new());
    let original = vec![
        message("u1", MessageRole::User, "old"),
        message("a1", MessageRole::Assistant, "old answer"),
        message("u2", MessageRole::User, "recent"),
    ];
    let mut state = AgentState {
        messages: original.clone(),
        ..AgentState::default()
    };
    let effect = AgentEffect::CompactMessages {
        compaction: MessageCompaction {
            summary_message: summary.clone(),
            retained_message_ids: vec!["u2".into()],
            dropped_message_ids: vec!["u1".into(), "a1".into()],
            tokens_before: 100,
            tokens_after: 20,
            metadata: Map::new(),
        },
    };
    let event = noloong_agent_core::AgentEvent {
        sequence: 1,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some(PHASE_CONTEXT_COMPACT.into()),
        kind: AgentEventKind::EffectCommitted { effect },
    };

    noloong_agent_core::apply_event(&mut state, &event)?;

    assert_eq!(state.messages, compacted_messages(summary, &original[2..]));
    let mut replay_events = Vec::new();
    for (index, message) in original.into_iter().enumerate() {
        replay_events.push(noloong_agent_core::AgentEvent {
            sequence: index as u64 + 1,
            run_id: "run-1".into(),
            turn_id: Some(1),
            phase: Some("test".into()),
            kind: AgentEventKind::EffectCommitted {
                effect: AgentEffect::AppendMessage { message },
            },
        });
    }
    replay_events.push(noloong_agent_core::AgentEvent {
        sequence: 4,
        ..event
    });
    assert_eq!(reduce_events(&replay_events)?.messages.len(), 2);
    Ok(())
}

#[test]
fn replace_messages_effect_replays_to_replacement_state() -> Result<()> {
    let original = vec![
        message("u1", MessageRole::User, "old"),
        message("a1", MessageRole::Assistant, "old answer"),
        message("u2", MessageRole::User, "recent"),
    ];
    let replacement_messages = vec![
        message(
            "replacement-summary",
            MessageRole::System,
            "replacement summary",
        ),
        message("u2", MessageRole::User, "recent"),
    ];
    let mut state = AgentState {
        messages: original.clone(),
        ..AgentState::default()
    };
    let effect = AgentEffect::ReplaceMessages {
        replacement: MessageReplacement {
            replacement_messages: replacement_messages.clone(),
            replaced_message_ids: original.iter().map(|message| message.id.clone()).collect(),
            tokens_before: 100,
            tokens_after: 20,
            metadata: Map::new(),
        },
    };
    let event = noloong_agent_core::AgentEvent {
        sequence: 1,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some(PHASE_CONTEXT_COMPACT.into()),
        kind: AgentEventKind::EffectCommitted { effect },
    };

    noloong_agent_core::apply_event(&mut state, &event)?;

    assert_eq!(state.messages, replacement_messages);
    Ok(())
}

#[test]
fn compact_messages_effect_rejects_invalid_ids() {
    let effect = AgentEffect::CompactMessages {
        compaction: MessageCompaction {
            summary_message: message("summary", MessageRole::System, "summary"),
            retained_message_ids: vec!["missing".into()],
            dropped_message_ids: Vec::new(),
            tokens_before: 10,
            tokens_after: 5,
            metadata: Map::new(),
        },
    };
    let event = noloong_agent_core::AgentEvent {
        sequence: 1,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some(PHASE_CONTEXT_COMPACT.into()),
        kind: AgentEventKind::EffectCommitted { effect },
    };
    let mut state = AgentState {
        messages: vec![message("u1", MessageRole::User, "hello")],
        ..AgentState::default()
    };

    assert!(noloong_agent_core::apply_event(&mut state, &event).is_err());
}

#[test]
fn replace_messages_effect_rejects_partial_state_coverage() {
    let effect = AgentEffect::ReplaceMessages {
        replacement: MessageReplacement {
            replacement_messages: vec![message("replacement", MessageRole::System, "summary")],
            replaced_message_ids: vec!["u1".into()],
            tokens_before: 10,
            tokens_after: 5,
            metadata: Map::new(),
        },
    };
    let event = noloong_agent_core::AgentEvent {
        sequence: 1,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some(PHASE_CONTEXT_COMPACT.into()),
        kind: AgentEventKind::EffectCommitted { effect },
    };
    let mut state = AgentState {
        messages: vec![
            message("u1", MessageRole::User, "hello"),
            message("a1", MessageRole::Assistant, "hi"),
        ],
        ..AgentState::default()
    };

    assert!(noloong_agent_core::apply_event(&mut state, &event).is_err());
}

#[test]
fn planner_skips_below_threshold() -> Result<()> {
    let messages = vec![message("u1", MessageRole::User, "short")];
    let decision = plan_compaction(
        &ContextCompactionConfig::new(10_000).reserve_tokens(1_000),
        &HeuristicTokenEstimator,
        &messages,
    )?;

    assert!(matches!(decision, CompactionDecision::Skip { .. }));
    Ok(())
}

#[test]
fn planner_never_keeps_tool_result_as_first_message() -> Result<()> {
    let messages = vec![
        message("u1", MessageRole::User, &"old ".repeat(40)),
        AgentMessage::assistant(
            "a1",
            vec![ContentBlock::ToolCall {
                tool_call: ToolCall {
                    id: "call-1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "path": "src/lib.rs" }),
                },
            }],
        ),
        AgentMessage::tool_result(
            "tr1",
            "call-1",
            "lookup",
            ToolOutput {
                content: vec![ContentBlock::Text {
                    text: "tool output ".repeat(80),
                }],
                details: json!({}),
                is_error: false,
                updates: Vec::new(),
            },
        ),
        message("u2", MessageRole::User, "recent"),
    ];

    let plan = compact_plan(&messages)?;

    assert_eq!(plan.retained_messages[0].id, "u2");
    assert!(plan.dropped_message_ids().contains(&"tr1".to_string()));
    Ok(())
}

#[test]
fn summary_serialization_truncates_top_level_tool_results_and_preserves_utf8() {
    let tool_output = AgentMessage::tool_result(
        "tr1",
        "call-1",
        "lookup",
        ToolOutput {
            content: vec![ContentBlock::Text {
                text: "tool output ".repeat(400),
            }],
            details: json!({}),
            is_error: false,
            updates: Vec::new(),
        },
    );
    let rendered = noloong_agent_core::serialize_messages_for_summary(&[tool_output]);

    assert!(rendered.contains("truncated"));
    assert!(!rendered.contains(&"tool output ".repeat(300)));

    let unicode_tool_output = AgentMessage::tool_result(
        "tr2",
        "call-2",
        "lookup",
        ToolOutput {
            content: vec![ContentBlock::Text {
                text: "\u{1f642}".repeat(3_000),
            }],
            details: json!({}),
            is_error: false,
            updates: Vec::new(),
        },
    );

    assert!(
        noloong_agent_core::serialize_messages_for_summary(&[unicode_tool_output])
            .contains("[... 1000 more characters truncated]")
    );
}

#[test]
fn planner_uses_previous_summary_and_split_turn_prefix() -> Result<()> {
    let mut previous_summary =
        compaction_summary_message("run-1", 1, "old summary".into(), Map::new());
    previous_summary.id = "summary-1".into();
    let messages = vec![
        previous_summary,
        message("u1", MessageRole::User, "turn start"),
        message("a1", MessageRole::Assistant, &"assistant ".repeat(80)),
    ];

    let plan = compact_plan(&messages)?;

    assert_eq!(plan.previous_summary.as_deref(), Some("old summary"));
    assert!(plan.is_split_turn);
    assert_eq!(plan.turn_prefix_messages[0].id, "u1");
    assert_eq!(plan.retained_messages[0].id, "a1");
    Ok(())
}

#[tokio::test]
async fn persistent_compaction_updates_state_and_provider_request() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compaction(
        provider.clone(),
        ContextCompactionMode::PersistentState,
        Arc::new(StaticSummarizer::new("summary").metadata("summary_source", json!("test"))),
    )?;
    let initial_state = long_state();

    let report = runtime
        .continue_from_state(initial_state, None, CancellationToken::new())
        .await?;

    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert_eq!(requests[0].messages[0].role, MessageRole::System);
    assert_text_absent(&requests[0].messages, "old old old");
    assert_text_present(&requests[0].messages, "summary");
    assert_text_present(&report.state.messages, "summary");
    assert_text_absent(&report.state.messages, "old old old");
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: AgentEffect::CompactMessages { .. }
            }
        )
    }));
    let compaction_metadata = report
        .events
        .iter()
        .find_map(|event| match &event.kind {
            AgentEventKind::EffectCommitted {
                effect: AgentEffect::CompactMessages { compaction },
            } => Some(&compaction.metadata),
            _ => None,
        })
        .expect("compaction effect");
    assert_eq!(compaction_metadata["summary_source"], json!("test"));
    assert_eq!(compaction_metadata["mode"], json!("persistent_state"));
    assert!(compaction_metadata["tokensBefore"].is_number());
    Ok(())
}

#[tokio::test]
async fn request_only_compaction_changes_request_without_changing_state_history() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compaction(
        provider.clone(),
        ContextCompactionMode::RequestOnly,
        Arc::new(StaticSummarizer::new("summary")),
    )?;
    let initial_state = long_state();

    let report = runtime
        .continue_from_state(initial_state, None, CancellationToken::new())
        .await?;

    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert_text_present(&requests[0].messages, "summary");
    assert_text_absent(&requests[0].messages, "old old old");
    assert_text_present(&report.state.messages, "old old old");
    assert!(!report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: AgentEffect::CompactMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn persistent_replacement_compaction_updates_state_and_provider_request() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compactor(
        provider.clone(),
        ContextCompactionMode::PersistentState,
        Arc::new(ReplacementCompactor),
    )?;
    let initial_state = long_state();

    let report = runtime
        .continue_from_state(initial_state, None, CancellationToken::new())
        .await?;

    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert_text_present(&requests[0].messages, "replacement summary");
    assert_text_absent(&requests[0].messages, "old old old");
    assert_text_present(&report.state.messages, "replacement summary");
    assert_text_absent(&report.state.messages, "old old old");
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: AgentEffect::ReplaceMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn request_only_replacement_compaction_changes_request_without_state_history() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compactor(
        provider.clone(),
        ContextCompactionMode::RequestOnly,
        Arc::new(ReplacementCompactor),
    )?;
    let initial_state = long_state();

    let report = runtime
        .continue_from_state(initial_state, None, CancellationToken::new())
        .await?;

    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert_text_present(&requests[0].messages, "replacement summary");
    assert_text_absent(&requests[0].messages, "old old old");
    assert_text_present(&report.state.messages, "old old old");
    assert!(!report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::EffectCommitted {
                effect: AgentEffect::ReplaceMessages { .. }
            }
        )
    }));
    Ok(())
}

#[tokio::test]
async fn compaction_noop_does_not_call_summarizer() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let summarizer = Arc::new(CountingSummarizer::default());
    let runtime = AgentRuntime::builder()
        .with_model_provider(provider)
        .with_context_compaction(
            ContextCompactionConfig::new(10_000).reserve_tokens(1_000),
            summarizer.clone(),
        )
        .max_turns(1)
        .build()?;

    runtime
        .continue_from_state(
            AgentState {
                messages: vec![message("u1", MessageRole::User, "short")],
                ..AgentState::default()
            },
            None,
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(summarizer.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn runtime_without_compaction_does_not_emit_context_compact_phase() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(CaptureModel::text("visible")))
        .max_turns(1)
        .build()?;

    let report = runtime.run("hello").await?;

    assert!(!report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::PhaseStarted { phase } if phase == PHASE_CONTEXT_COMPACT
        )
    }));
    Ok(())
}

#[tokio::test]
async fn compaction_error_stops_before_model_provider() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compaction(
        provider.clone(),
        ContextCompactionMode::PersistentState,
        Arc::new(FailingSummarizer),
    )?;

    let result = runtime
        .continue_from_state(long_state(), None, CancellationToken::new())
        .await;

    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn compaction_cancellation_stops_before_model_provider() -> Result<()> {
    let provider = Arc::new(CaptureModel::text("visible"));
    let runtime = runtime_with_compaction(
        provider.clone(),
        ContextCompactionMode::PersistentState,
        Arc::new(CancellingSummarizer),
    )?;

    let result = runtime
        .continue_from_state(long_state(), None, CancellationToken::new())
        .await;

    assert!(matches!(result, Err(AgentCoreError::Aborted)));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn model_backed_summarizer_serializes_conversation_into_prompt() -> Result<()> {
    let provider = Arc::new(SequenceModel::new(vec![vec![
        ModelStreamEvent::TextDelta {
            text: "generated summary".into(),
        },
        ModelStreamEvent::Finished {
            stop_reason: StopReason::Stop,
        },
    ]]));
    let summarizer = ModelBackedCompactionSummarizer::new(
        ModelBackedCompactionSummarizerConfig::new("summary"),
        provider.clone(),
    );

    let result = summarizer
        .summarize(
            summary_request(None, vec![message("u1", MessageRole::User, "hello")]),
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(result.summary, "generated summary");
    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert!(requests[0].tools.is_empty());
    assert_text_present(&requests[0].messages, "<conversation>");
    assert_text_present(&requests[0].messages, "[user]: hello");
    Ok(())
}

#[tokio::test]
async fn model_backed_summarizer_updates_previous_summary_and_split_prefix() -> Result<()> {
    let provider = Arc::new(SequenceModel::new(vec![
        vec![
            ModelStreamEvent::TextDelta {
                text: "updated history".into(),
            },
            ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            },
        ],
        vec![
            ModelStreamEvent::TextDelta {
                text: "prefix summary".into(),
            },
            ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            },
        ],
    ]));
    let summarizer = ModelBackedCompactionSummarizer::new(
        ModelBackedCompactionSummarizerConfig::new("summary"),
        provider.clone(),
    );
    let mut request = summary_request(
        Some("previous"),
        vec![message("u1", MessageRole::User, "new info")],
    );
    request.turn_prefix_messages = vec![message("u2", MessageRole::User, "prefix")];

    let result = summarizer
        .summarize(request, CancellationToken::new())
        .await?;

    assert!(result.summary.contains("updated history"));
    assert!(result.summary.contains("prefix summary"));
    let requests = provider.requests.lock().expect("requests lock poisoned");
    assert_text_present(&requests[0].messages, "<previous-summary>");
    assert_eq!(requests.len(), 2);
    Ok(())
}

#[tokio::test]
async fn model_backed_summarizer_fails_on_failed_or_empty_output() -> Result<()> {
    let failed_provider = Arc::new(SequenceModel::new(vec![vec![ModelStreamEvent::Failed {
        error: "boom".into(),
    }]]));
    let failed = ModelBackedCompactionSummarizer::new(
        ModelBackedCompactionSummarizerConfig::new("summary"),
        failed_provider,
    )
    .summarize(
        summary_request(None, vec![message("u1", MessageRole::User, "hello")]),
        CancellationToken::new(),
    )
    .await;
    assert!(failed.is_err());

    let empty_provider = Arc::new(SequenceModel::new(vec![vec![ModelStreamEvent::Finished {
        stop_reason: StopReason::Stop,
    }]]));
    let empty = ModelBackedCompactionSummarizer::new(
        ModelBackedCompactionSummarizerConfig::new("summary"),
        empty_provider,
    )
    .summarize(
        summary_request(None, vec![message("u1", MessageRole::User, "hello")]),
        CancellationToken::new(),
    )
    .await;
    assert!(empty.is_err());
    Ok(())
}

fn compact_plan(messages: &[AgentMessage]) -> Result<noloong_agent_core::CompactionPlan> {
    match plan_compaction(
        &ContextCompactionConfig::new(64)
            .reserve_tokens(8)
            .keep_recent_tokens(10),
        &HeuristicTokenEstimator,
        messages,
    )? {
        CompactionDecision::Compact(plan) => Ok(plan),
        CompactionDecision::Skip { estimated_tokens } => Err(AgentCoreError::Phase(format!(
            "expected compaction, got skip with {estimated_tokens} tokens"
        ))),
    }
}

fn runtime_with_compaction(
    provider: Arc<CaptureModel>,
    mode: ContextCompactionMode,
    summarizer: Arc<dyn CompactionSummarizer>,
) -> Result<AgentRuntime> {
    AgentRuntime::builder()
        .with_model_provider(provider)
        .with_context_compaction(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10)
                .mode(mode),
            summarizer,
        )
        .max_turns(1)
        .build()
}

fn runtime_with_compactor(
    provider: Arc<CaptureModel>,
    mode: ContextCompactionMode,
    compactor: Arc<dyn ContextCompactor>,
) -> Result<AgentRuntime> {
    AgentRuntime::builder()
        .with_model_provider(provider)
        .with_context_compactor(
            ContextCompactionConfig::new(64)
                .reserve_tokens(8)
                .keep_recent_tokens(10)
                .mode(mode),
            compactor,
        )
        .max_turns(1)
        .build()
}

fn long_state() -> AgentState {
    AgentState {
        messages: vec![
            message("u1", MessageRole::User, &"old ".repeat(80)),
            message("a1", MessageRole::Assistant, &"old answer ".repeat(80)),
            message("u2", MessageRole::User, "recent"),
        ],
        ..AgentState::default()
    }
}

fn summary_request(
    previous_summary: Option<&str>,
    messages: Vec<AgentMessage>,
) -> CompactionSummaryRequest {
    CompactionSummaryRequest {
        run_id: "run-1".into(),
        turn_id: 1,
        previous_summary: previous_summary.map(Into::into),
        messages_to_summarize: messages,
        turn_prefix_messages: Vec::new(),
        token_budget: 128,
        metadata: Map::new(),
    }
}

fn message(id: &str, role: MessageRole, text: &str) -> AgentMessage {
    AgentMessage {
        id: id.into(),
        role,
        content: vec![ContentBlock::Text { text: text.into() }],
        metadata: Map::new(),
    }
}

fn assert_text_present(messages: &[AgentMessage], expected: &str) {
    assert!(
        rendered_text(messages).contains(expected),
        "expected `{expected}` in messages: {messages:#?}"
    );
}

fn assert_text_absent(messages: &[AgentMessage], unexpected: &str) {
    assert!(
        !rendered_text(messages).contains(unexpected),
        "did not expect `{unexpected}` in messages: {messages:#?}"
    );
}

fn rendered_text(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Default)]
struct CountingSummarizer {
    calls: AtomicUsize,
}

struct ReplacementCompactor;

impl ContextCompactor for ReplacementCompactor {
    fn id(&self) -> &str {
        "replacement"
    }

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ContextCompactionOutput> {
        Box::pin(async move {
            let mut replacement_messages = vec![message(
                "replacement-summary",
                MessageRole::System,
                "replacement summary",
            )];
            replacement_messages.extend(request.retained_messages);
            Ok(ContextCompactionOutput::replacement(replacement_messages))
        })
    }
}

impl CompactionSummarizer for CountingSummarizer {
    fn id(&self) -> &str {
        "counting"
    }

    fn summarize<'a>(
        &'a self,
        _request: CompactionSummaryRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(CompactionSummaryResult {
                summary: "summary".into(),
                metadata: Map::new(),
            })
        })
    }
}

struct StaticSummarizer {
    summary: String,
    metadata: Map<String, serde_json::Value>,
}

impl StaticSummarizer {
    fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            metadata: Map::new(),
        }
    }

    fn metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl CompactionSummarizer for StaticSummarizer {
    fn id(&self) -> &str {
        "static"
    }

    fn summarize<'a>(
        &'a self,
        _request: CompactionSummaryRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async move {
            Ok(CompactionSummaryResult {
                summary: self.summary.clone(),
                metadata: self.metadata.clone(),
            })
        })
    }
}

struct FailingSummarizer;

impl CompactionSummarizer for FailingSummarizer {
    fn id(&self) -> &str {
        "failing"
    }

    fn summarize<'a>(
        &'a self,
        _request: CompactionSummaryRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async { Err(AgentCoreError::Phase("summary failed".into())) })
    }
}

struct CancellingSummarizer;

impl CompactionSummarizer for CancellingSummarizer {
    fn id(&self) -> &str {
        "cancelling"
    }

    fn summarize<'a>(
        &'a self,
        _request: CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async move {
            cancellation.cancel();
            cancellation.throw_if_cancelled()?;
            Ok(CompactionSummaryResult {
                summary: "unreachable".into(),
                metadata: Map::new(),
            })
        })
    }
}

struct CaptureModel {
    requests: Arc<Mutex<Vec<ModelRequest>>>,
    calls: AtomicUsize,
    events: Vec<ModelStreamEvent>,
}

impl CaptureModel {
    fn text(text: impl Into<String>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            calls: AtomicUsize::new(0),
            events: vec![
                ModelStreamEvent::TextDelta { text: text.into() },
                ModelStreamEvent::Finished {
                    stop_reason: StopReason::Stop,
                },
            ],
        }
    }
}

impl ModelProvider for CaptureModel {
    fn id(&self) -> &str {
        "capture"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests
                .lock()
                .expect("requests lock poisoned")
                .push(request);
            for event in &self.events {
                stream(event.clone()).await?;
            }
            Ok(self.events.clone())
        })
    }
}

struct SequenceModel {
    requests: Arc<Mutex<Vec<ModelRequest>>>,
    responses: Arc<Mutex<VecDeque<Vec<ModelStreamEvent>>>>,
}

impl SequenceModel {
    fn new(responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(responses.into())),
        }
    }
}

impl ModelProvider for SequenceModel {
    fn id(&self) -> &str {
        "sequence"
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            self.requests
                .lock()
                .expect("requests lock poisoned")
                .push(request);
            let events = self
                .responses
                .lock()
                .expect("responses lock poisoned")
                .pop_front()
                .unwrap_or_default();
            for event in &events {
                stream(event.clone()).await?;
            }
            Ok(events)
        })
    }
}

#[test]
fn token_estimator_covers_structured_blocks() {
    let message = AgentMessage {
        id: "a1".into(),
        role: MessageRole::Assistant,
        content: vec![
            ContentBlock::Json {
                value: json!({ "key": "value" }),
            },
            ContentBlock::Thinking {
                thinking: noloong_agent_core::ThinkingBlock::from_text("think"),
            },
            ContentBlock::Media {
                media: MediaBlock::inline_base64(MediaKind::Image, "abc"),
            },
            ContentBlock::ToolCall {
                tool_call: ToolCall {
                    id: "call-1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "path": "src/lib.rs" }),
                },
            },
        ],
        metadata: Map::new(),
    };

    assert!(HeuristicTokenEstimator.estimate_message_tokens(&message) > 0);
}
