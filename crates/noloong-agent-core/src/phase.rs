use crate::compaction::{
    COMPACTION_METADATA_IS_SPLIT_TURN_KEY, COMPACTION_METADATA_MODE_KEY,
    COMPACTION_METADATA_TOKENS_BEFORE_KEY,
};
use crate::runtime::ToolRuntimeHandles;
use crate::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, AfterToolCallContext, AgentEffect, AgentMessage,
    BeforeAssistantCommitHookContext, BeforeAssistantCommitHookResult,
    BeforeModelRequestHookContext, BeforeModelRequestHookResult, BeforeToolCallContext,
    BeforeToolCallResult, CompactionDecision, ContentBlock, ContextCompactionMode, ContextRequest,
    MediaBlock, MediaDelta, MediaSource, MessageCompaction, ModelRequest, ModelStreamEvent,
    PhaseHook, Result, RunPauseReason, ThinkingBlock, ToolApprovalContinuation,
    ToolApprovalPreflight, ToolApprovalPreflightStatus, ToolApprovalRequest,
    ToolApprovalResolution, ToolCall, ToolExecutionMode, ToolOutput, ToolPermissionAudit,
    ToolPermissionDecision, ToolPermissionDecisionRecord, ToolPermissionOutcome, TurnDecision,
    compacted_messages, compaction_summary_message, plan_compaction,
    provider_utils::collect_model_stream,
    providers::{BoxFuture, CancellationToken, ModelStreamSink},
};
use crate::{AgentRuntime, AgentState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;

pub const PHASE_INPUT_INGEST: &str = "input.ingest";
pub const PHASE_CONTEXT_PREPARE: &str = "context.prepare";
pub const PHASE_CONTEXT_COMPACT: &str = "context.compact";
pub const PHASE_MODEL_REQUEST_PREPARE: &str = "model.request.prepare";
pub const PHASE_MODEL_STREAM: &str = "model.stream";
pub const PHASE_ASSISTANT_COMMIT: &str = "assistant.commit";
pub const PHASE_TOOL_CALL_RESOLVE: &str = "tool.call.resolve";
pub const PHASE_TOOL_EXECUTE: &str = "tool.execute";
pub const PHASE_TURN_DECISION: &str = "turn.decision";

pub trait PhaseNode: Send + Sync {
    fn id(&self) -> &str;
    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput>;
}

pub struct PhaseContext<'a> {
    pub runtime: &'a AgentRuntime,
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: AgentState,
    pub scratch: PhaseScratch,
    pub cancellation: CancellationToken,
    pub model_stream_sink: Option<ModelStreamSink>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhaseScratch {
    #[serde(default)]
    pub input: Option<AgentMessage>,
    #[serde(default)]
    pub model_request: Option<ModelRequest>,
    #[serde(default)]
    pub request_messages_override: Option<Vec<AgentMessage>>,
    #[serde(default)]
    pub model_events: Vec<ModelStreamEvent>,
    #[serde(default)]
    pub assistant_message: Option<AgentMessage>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub decision: Option<TurnDecision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseOutput {
    #[serde(default)]
    pub scratch: PhaseScratch,
    #[serde(default)]
    pub effects: Vec<AgentEffect>,
    #[serde(default)]
    pub stream_events: Vec<ModelStreamEvent>,
    #[serde(default)]
    pub resolved_tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub completed_tool_outputs: Vec<(ToolCall, ToolOutput)>,
    #[serde(default)]
    pub tool_permission_audits: Vec<ToolPermissionAudit>,
    #[serde(default)]
    pub completed_tool_permission_audits: Vec<ToolPermissionAudit>,
    #[serde(default)]
    pub tool_approval_requests: Vec<ToolApprovalRequest>,
    #[serde(default)]
    pub pause: Option<RunPauseReason>,
}

impl PhaseOutput {
    pub fn from_scratch(scratch: PhaseScratch) -> Self {
        Self {
            scratch,
            effects: Vec::new(),
            stream_events: Vec::new(),
            resolved_tool_calls: Vec::new(),
            tool_outputs: Vec::new(),
            completed_tool_outputs: Vec::new(),
            tool_permission_audits: Vec::new(),
            completed_tool_permission_audits: Vec::new(),
            tool_approval_requests: Vec::new(),
            pause: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum StandardPhase {
    InputIngest,
    ContextPrepare,
    ContextCompact,
    ModelRequestPrepare,
    ModelStream,
    AssistantCommit,
    ToolCallResolve,
    ToolExecute,
    TurnDecision,
}

impl PhaseNode for StandardPhase {
    fn id(&self) -> &str {
        match self {
            Self::InputIngest => PHASE_INPUT_INGEST,
            Self::ContextPrepare => PHASE_CONTEXT_PREPARE,
            Self::ContextCompact => PHASE_CONTEXT_COMPACT,
            Self::ModelRequestPrepare => PHASE_MODEL_REQUEST_PREPARE,
            Self::ModelStream => PHASE_MODEL_STREAM,
            Self::AssistantCommit => PHASE_ASSISTANT_COMMIT,
            Self::ToolCallResolve => PHASE_TOOL_CALL_RESOLVE,
            Self::ToolExecute => PHASE_TOOL_EXECUTE,
            Self::TurnDecision => PHASE_TURN_DECISION,
        }
    }

    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput> {
        Box::pin(async move {
            match self {
                Self::InputIngest => input_ingest(context).await,
                Self::ContextPrepare => context_prepare(context).await,
                Self::ContextCompact => context_compact(context).await,
                Self::ModelRequestPrepare => model_request_prepare(context).await,
                Self::ModelStream => model_stream(context).await,
                Self::AssistantCommit => assistant_commit(context).await,
                Self::ToolCallResolve => tool_call_resolve(context).await,
                Self::ToolExecute => tool_execute(context).await,
                Self::TurnDecision => turn_decision(context).await,
            }
        })
    }
}

async fn input_ingest(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    if context.turn_id == 1
        && let Some(input) = output.scratch.input.clone()
    {
        output
            .effects
            .push(AgentEffect::AppendMessage { message: input });
    }
    Ok(output)
}

async fn context_prepare(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    for provider in context.runtime.context_providers() {
        context.cancellation.throw_if_cancelled()?;
        let request = ContextRequest {
            run_id: context.run_id.to_string(),
            turn_id: context.turn_id,
            state: context.state.clone(),
        };
        output.effects.extend(
            provider
                .prepare_context(request, context.cancellation.clone())
                .await?,
        );
    }
    Ok(output)
}

async fn context_compact(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let PhaseContext {
        runtime,
        run_id,
        turn_id,
        state,
        scratch,
        cancellation,
        ..
    } = context;
    let mut output = PhaseOutput::from_scratch(scratch);
    let Some(compaction) = runtime.context_compaction() else {
        return Ok(output);
    };
    cancellation.throw_if_cancelled()?;
    let decision = plan_compaction(
        &compaction.config,
        compaction.estimator.as_ref(),
        &state.messages,
    )?;
    let CompactionDecision::Compact(plan) = decision else {
        return Ok(output);
    };

    let retained_message_ids = plan.retained_message_ids().to_vec();
    let dropped_message_ids = plan.dropped_message_ids().to_vec();
    let crate::CompactionPlan {
        previous_summary,
        messages_to_summarize,
        turn_prefix_messages,
        retained_messages,
        tokens_before,
        is_split_turn,
        ..
    } = plan;
    let request = crate::CompactionSummaryRequest {
        run_id: run_id.to_string(),
        turn_id,
        previous_summary,
        messages_to_summarize,
        turn_prefix_messages,
        token_budget: compaction.config.reserve_tokens,
        metadata: compaction.config.metadata.clone(),
    };
    let summary_result = compaction
        .summarizer
        .summarize(request, cancellation.clone())
        .await?;
    if summary_result.summary.trim().is_empty() {
        return Err(crate::AgentCoreError::Phase(
            "compaction summarizer returned an empty summary".into(),
        ));
    }
    let mut summary_metadata = compaction.config.metadata.clone();
    summary_metadata.extend(summary_result.metadata);
    summary_metadata.insert(
        COMPACTION_METADATA_MODE_KEY.into(),
        serde_json::json!(compaction.config.mode),
    );
    summary_metadata.insert(
        COMPACTION_METADATA_TOKENS_BEFORE_KEY.into(),
        serde_json::json!(tokens_before),
    );
    summary_metadata.insert(
        COMPACTION_METADATA_IS_SPLIT_TURN_KEY.into(),
        serde_json::json!(is_split_turn),
    );
    let summary_message =
        compaction_summary_message(run_id, turn_id, summary_result.summary, summary_metadata);
    let compacted_messages = compacted_messages(summary_message.clone(), &retained_messages);
    let tokens_after = compaction
        .estimator
        .estimate_messages_tokens(&compacted_messages);

    match compaction.config.mode {
        ContextCompactionMode::PersistentState => {
            output.effects.push(AgentEffect::CompactMessages {
                compaction: MessageCompaction {
                    summary_message,
                    retained_message_ids,
                    dropped_message_ids,
                    tokens_before,
                    tokens_after,
                    metadata: compaction.config.metadata.clone(),
                },
            });
        }
        ContextCompactionMode::RequestOnly => {
            output.scratch.request_messages_override = Some(compacted_messages);
        }
    }
    Ok(output)
}

async fn model_request_prepare(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let PhaseContext {
        runtime,
        run_id,
        turn_id,
        state,
        scratch,
        cancellation,
        ..
    } = context;
    let mut output = PhaseOutput::from_scratch(scratch);
    let context_map = state
        .context
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let request_messages = output
        .scratch
        .request_messages_override
        .take()
        .unwrap_or_else(|| state.messages.clone());
    let request = ModelRequest {
        run_id: run_id.to_string(),
        turn_id,
        messages: request_messages,
        context: context_map,
        tools: runtime
            .tool_specs()
            .into_iter()
            .map(|tool| tool.spec())
            .collect(),
        metadata: Default::default(),
    };
    let hook_runner = PhaseHookRunner::new(
        runtime.phase_hooks(),
        run_id,
        turn_id,
        &state,
        &cancellation,
    );
    output.scratch.model_request = Some(hook_runner.before_model_request(request).await?);
    Ok(output)
}

async fn model_stream(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let PhaseContext {
        runtime,
        run_id,
        turn_id,
        state,
        scratch,
        cancellation,
        model_stream_sink,
    } = context;
    let mut output = PhaseOutput::from_scratch(scratch);
    cancellation.throw_if_cancelled()?;
    let provider = runtime.default_model_provider()?;
    let request = output
        .scratch
        .model_request
        .clone()
        .ok_or_else(|| crate::AgentCoreError::Phase("model request was not prepared".into()))?;
    let hook_runner = PhaseHookRunner::new(
        runtime.phase_hooks(),
        run_id,
        turn_id,
        &state,
        &cancellation,
    );
    let request_for_hooks = hook_runner.has_hooks().then(|| request.clone());
    let stream = collect_model_stream(
        provider.as_ref(),
        request,
        model_stream_sink,
        cancellation.clone(),
    )
    .await?;
    let events = stream.events;
    if !stream.emitted_events {
        output.stream_events = events.clone();
    }
    output.scratch.model_events = match request_for_hooks {
        Some(request) => hook_runner.after_model_request(&request, events).await?,
        None => events,
    };
    Ok(output)
}

struct PhaseHookRunner<'a> {
    hooks: &'a [Arc<dyn PhaseHook>],
    run_id: &'a str,
    turn_id: u64,
    state: &'a AgentState,
    cancellation: &'a CancellationToken,
}

impl<'a> PhaseHookRunner<'a> {
    fn new(
        hooks: &'a [Arc<dyn PhaseHook>],
        run_id: &'a str,
        turn_id: u64,
        state: &'a AgentState,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            hooks,
            run_id,
            turn_id,
            state,
            cancellation,
        }
    }

    fn has_hooks(&self) -> bool {
        !self.hooks.is_empty()
    }

    async fn before_model_request(&self, mut request: ModelRequest) -> Result<ModelRequest> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(BeforeModelRequestHookResult { request: next }) = hook
                .before_model_request(
                    BeforeModelRequestHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        request: &request,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                request = next;
            }
        }
        Ok(request)
    }

    async fn after_model_request(
        &self,
        request: &ModelRequest,
        mut events: Vec<ModelStreamEvent>,
    ) -> Result<Vec<ModelStreamEvent>> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(AfterModelRequestHookResult { events: next }) = hook
                .after_model_request(
                    AfterModelRequestHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        request,
                        events: &events,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                events = next;
            }
        }
        Ok(events)
    }

    async fn before_assistant_commit(
        &self,
        mut events: Vec<ModelStreamEvent>,
    ) -> Result<Vec<ModelStreamEvent>> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(BeforeAssistantCommitHookResult { events: next }) = hook
                .before_assistant_commit(
                    BeforeAssistantCommitHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        events: &events,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                events = next;
            }
        }
        Ok(events)
    }

    async fn after_assistant_commit(&self, mut message: AgentMessage) -> Result<AgentMessage> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(AfterAssistantCommitHookResult { message: next }) = hook
                .after_assistant_commit(
                    AfterAssistantCommitHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        message: &message,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                message = next;
            }
        }
        Ok(message)
    }
}

async fn assistant_commit(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let PhaseContext {
        runtime,
        run_id,
        turn_id,
        state,
        scratch,
        cancellation,
        ..
    } = context;
    let mut output = PhaseOutput::from_scratch(scratch);
    let hook_runner = PhaseHookRunner::new(
        runtime.phase_hooks(),
        run_id,
        turn_id,
        &state,
        &cancellation,
    );
    if hook_runner.has_hooks() {
        output.scratch.model_events = hook_runner
            .before_assistant_commit(output.scratch.model_events)
            .await?;
    }
    let mut thinking: Option<ThinkingBlock> = None;
    let mut media: Option<MediaBlock> = None;
    let mut text = String::new();
    let mut content = Vec::new();
    for event in &output.scratch.model_events {
        match event {
            ModelStreamEvent::ThinkingDelta { delta } => {
                flush_media(&mut content, &mut media);
                flush_text(&mut content, &mut text);
                if !delta.is_empty() {
                    if thinking
                        .as_ref()
                        .is_some_and(|block| block.kind != delta.kind)
                    {
                        flush_thinking(&mut content, &mut thinking);
                    }
                    match &mut thinking {
                        Some(block) => block.apply_delta(delta),
                        None => thinking = Some(ThinkingBlock::from_delta(delta)),
                    }
                }
            }
            ModelStreamEvent::TextDelta { text: delta } => {
                flush_thinking(&mut content, &mut thinking);
                flush_media(&mut content, &mut media);
                text.push_str(delta);
            }
            ModelStreamEvent::MediaDelta { delta } => {
                if delta.is_empty() {
                    continue;
                }
                flush_thinking(&mut content, &mut thinking);
                flush_text(&mut content, &mut text);
                if media
                    .as_ref()
                    .is_some_and(|block| media_delta_starts_new_block(block, delta))
                {
                    flush_media(&mut content, &mut media);
                }
                match &mut media {
                    Some(block) => block.apply_delta(delta),
                    None => media = MediaBlock::from_delta(delta),
                }
                if delta.done {
                    flush_media(&mut content, &mut media);
                }
            }
            ModelStreamEvent::ToolCall { tool_call } => {
                flush_thinking(&mut content, &mut thinking);
                flush_text(&mut content, &mut text);
                flush_media(&mut content, &mut media);
                content.push(ContentBlock::ToolCall {
                    tool_call: tool_call.clone(),
                });
            }
            ModelStreamEvent::Failed { error } => {
                return Err(crate::AgentCoreError::Phase(format!(
                    "model stream failed: {error}"
                )));
            }
            ModelStreamEvent::Started { .. } | ModelStreamEvent::Finished { .. } => {}
        }
    }
    flush_thinking(&mut content, &mut thinking);
    flush_text(&mut content, &mut text);
    flush_media(&mut content, &mut media);
    let message = AgentMessage::assistant(
        format!("assistant-{}-{}", context.run_id, context.turn_id),
        content,
    );
    let message = if hook_runner.has_hooks() {
        hook_runner.after_assistant_commit(message).await?
    } else {
        message
    };
    output.effects.push(AgentEffect::AppendMessage {
        message: message.clone(),
    });
    output.scratch.assistant_message = Some(message);
    Ok(output)
}

fn flush_thinking(content: &mut Vec<ContentBlock>, thinking: &mut Option<ThinkingBlock>) {
    if let Some(thinking) = thinking.take()
        && !thinking.is_empty()
    {
        content.push(ContentBlock::Thinking { thinking });
    }
}

fn flush_text(content: &mut Vec<ContentBlock>, text: &mut String) {
    if !text.is_empty() {
        content.push(ContentBlock::Text {
            text: std::mem::take(text),
        });
    }
}

fn flush_media(content: &mut Vec<ContentBlock>, media: &mut Option<MediaBlock>) {
    if let Some(media) = media.take() {
        content.push(ContentBlock::Media { media });
    }
}

fn media_delta_starts_new_block(block: &MediaBlock, delta: &MediaDelta) -> bool {
    if block.kind != delta.kind {
        return true;
    }
    let Some(source) = &delta.source else {
        return false;
    };
    if block.source == *source {
        return false;
    }
    !matches!(&block.source, MediaSource::Inline { .. })
}

async fn tool_call_resolve(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    if let Some(message) = &output.scratch.assistant_message {
        for block in &message.content {
            if let ContentBlock::ToolCall { tool_call } = block {
                output.scratch.tool_calls.push(tool_call.clone());
                output.resolved_tool_calls.push(tool_call.clone());
            }
        }
    }
    Ok(output)
}

async fn tool_execute(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch.clone());
    let tool_calls = output.scratch.tool_calls.clone();
    if tool_calls.is_empty() {
        return Ok(output);
    }

    let tool_execution_mode = tool_execution_mode_for_calls(&context, &tool_calls);
    let handles = context.runtime.tool_handles();
    let (preflights, approval_requests) = prepare_tool_preflights(
        handles.clone(),
        context.run_id,
        context.turn_id,
        &context.state,
        &tool_calls,
        tool_execution_mode,
        context.cancellation.clone(),
    )
    .await?;
    if !approval_requests.is_empty() {
        set_tool_approval_pause(
            &mut output,
            context.run_id,
            context.turn_id,
            tool_execution_mode,
            preflights,
            approval_requests,
        );
        return Ok(output);
    }

    let source_order_outputs = if tool_execution_mode == ToolExecutionMode::Sequential {
        execute_prepared_tools_sequential(&context, preflights, &mut output).await?
    } else {
        execute_prepared_tools_parallel(&context, preflights, &mut output).await?
    };

    append_tool_execution_effects(&mut output, context.run_id, source_order_outputs);
    Ok(output)
}

pub(crate) async fn resume_tool_approval_continuation(
    runtime: &AgentRuntime,
    continuation: ToolApprovalContinuation,
    state: AgentState,
    resolutions: Vec<ToolApprovalResolution>,
    cancellation: CancellationToken,
) -> Result<PhaseOutput> {
    if continuation.phase != PHASE_TOOL_EXECUTE {
        return Err(crate::AgentCoreError::Phase(format!(
            "cannot resume approval continuation for phase {}",
            continuation.phase
        )));
    }

    let preflight_context = ToolPreflightContext {
        handles: runtime.tool_handles(),
        run_id: continuation.run_id.clone(),
        turn_id: continuation.turn_id,
        state: state.clone(),
        cancellation: cancellation.clone(),
    };
    let mut resolution_by_id = resolutions
        .into_iter()
        .map(|resolution| (resolution.approval_id.clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    let mut preflights = Vec::with_capacity(continuation.preflights.len());
    let mut approval_requests = Vec::new();

    for preflight in continuation.preflights {
        let ToolApprovalPreflight {
            tool_call,
            mut permission_audit,
            status,
        } = preflight;
        match status {
            ToolApprovalPreflightStatus::Pending {
                approval_id,
                hook_index,
                hook_id,
            } => {
                let resolution = resolution_by_id.remove(&approval_id).ok_or_else(|| {
                    crate::AgentCoreError::Phase(format!(
                        "missing tool approval resolution for {approval_id}"
                    ))
                })?;
                permission_audit
                    .decisions
                    .push(ToolPermissionDecisionRecord {
                        hook_id,
                        decision: resolution.decision.clone(),
                    });
                if matches!(resolution.decision.outcome, ToolPermissionOutcome::Deny) {
                    preflights.push(ToolApprovalPreflight {
                        tool_call,
                        permission_audit,
                        status: ToolApprovalPreflightStatus::Denied {
                            decision: resolution.decision,
                        },
                    });
                    continue;
                }

                let (prepared, approval_request) = prepare_one_tool_call(
                    &preflight_context,
                    tool_call,
                    hook_index + 1,
                    Some(permission_audit),
                )
                .await?;
                if let Some(approval_request) = approval_request {
                    approval_requests.push(approval_request);
                }
                preflights.push(prepared);
            }
            status => {
                preflights.push(ToolApprovalPreflight {
                    tool_call,
                    permission_audit,
                    status,
                });
            }
        }
    }

    if !resolution_by_id.is_empty() {
        let approval_ids = resolution_by_id.keys().cloned().collect::<Vec<_>>();
        return Err(crate::AgentCoreError::Phase(format!(
            "unknown tool approval resolution ids: {}",
            approval_ids.join(", ")
        )));
    }

    let mut output = PhaseOutput::from_scratch(continuation.scratch.clone());
    if !approval_requests.is_empty() {
        set_tool_approval_pause(
            &mut output,
            &continuation.run_id,
            continuation.turn_id,
            continuation.tool_execution_mode,
            preflights,
            approval_requests,
        );
        return Ok(output);
    }

    let context = PhaseContext {
        runtime,
        run_id: &continuation.run_id,
        turn_id: continuation.turn_id,
        state,
        scratch: continuation.scratch,
        cancellation,
        model_stream_sink: None,
    };
    let source_order_outputs = if continuation.tool_execution_mode == ToolExecutionMode::Sequential
    {
        execute_prepared_tools_sequential(&context, preflights, &mut output).await?
    } else {
        execute_prepared_tools_parallel(&context, preflights, &mut output).await?
    };
    append_tool_execution_effects(&mut output, &continuation.run_id, source_order_outputs);
    Ok(output)
}

fn set_tool_approval_pause(
    output: &mut PhaseOutput,
    run_id: &str,
    turn_id: u64,
    tool_execution_mode: ToolExecutionMode,
    preflights: Vec<ToolApprovalPreflight>,
    approval_requests: Vec<ToolApprovalRequest>,
) {
    output.tool_approval_requests = approval_requests;
    output.pause = Some(RunPauseReason::ToolApproval {
        continuation: ToolApprovalContinuation {
            run_id: run_id.to_string(),
            turn_id,
            phase: PHASE_TOOL_EXECUTE.into(),
            scratch: output.scratch.clone(),
            tool_execution_mode,
            preflights,
        },
    });
}

fn append_tool_execution_effects(
    output: &mut PhaseOutput,
    run_id: &str,
    source_order_outputs: Vec<(ToolCall, ToolOutput)>,
) {
    for (tool_call, tool_output) in source_order_outputs {
        output
            .scratch
            .tool_outputs
            .push((tool_call.clone(), tool_output.clone()));
        output
            .tool_outputs
            .push((tool_call.clone(), tool_output.clone()));
        output.effects.push(AgentEffect::AppendMessage {
            message: AgentMessage::tool_result(
                format!("tool-result-{}-{}", run_id, tool_call.id),
                tool_call.id,
                tool_call.name,
                tool_output,
            ),
        });
    }
}

fn tool_execution_mode_for_calls(
    context: &PhaseContext<'_>,
    tool_calls: &[ToolCall],
) -> ToolExecutionMode {
    if context.runtime.tool_execution_mode() == ToolExecutionMode::Sequential
        || tool_calls.iter().any(|tool_call| {
            context
                .runtime
                .tool(&tool_call.name)
                .ok()
                .and_then(|tool| tool.spec().execution_mode)
                == Some(ToolExecutionMode::Sequential)
        })
    {
        ToolExecutionMode::Sequential
    } else {
        ToolExecutionMode::Parallel
    }
}

async fn prepare_tool_preflights(
    handles: ToolRuntimeHandles,
    run_id: &str,
    turn_id: u64,
    state: &AgentState,
    tool_calls: &[ToolCall],
    tool_execution_mode: ToolExecutionMode,
    cancellation: CancellationToken,
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let context = ToolPreflightContext {
        handles,
        run_id: run_id.to_string(),
        turn_id,
        state: state.clone(),
        cancellation,
    };
    if tool_execution_mode == ToolExecutionMode::Parallel {
        return prepare_tool_preflights_parallel(context, tool_calls).await;
    }
    prepare_tool_preflights_sequential(&context, tool_calls).await
}

async fn prepare_tool_preflights_sequential(
    context: &ToolPreflightContext,
    tool_calls: &[ToolCall],
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let mut preflights = Vec::with_capacity(tool_calls.len());
    let mut approval_requests = Vec::new();
    for tool_call in tool_calls {
        let (preflight, approval_request) =
            prepare_one_tool_call(context, tool_call.clone(), 0, None).await?;
        if let Some(approval_request) = approval_request {
            approval_requests.push(approval_request);
        }
        preflights.push(preflight);
    }
    Ok((preflights, approval_requests))
}

async fn prepare_tool_preflights_parallel(
    context: ToolPreflightContext,
    tool_calls: &[ToolCall],
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let (sender, mut receiver) = mpsc::channel(tool_calls.len());
    let tool_call_count = tool_calls.len();
    for (index, tool_call) in tool_calls.iter().cloned().enumerate() {
        let sender = sender.clone();
        let context = context.clone();
        tokio::spawn(async move {
            let result = prepare_one_tool_call(&context, tool_call, 0, None).await;
            let _ = sender.send((index, result)).await;
        });
    }
    drop(sender);

    let mut source_order_preflights = vec![None; tool_call_count];
    while let Some((index, result)) = receiver.recv().await {
        source_order_preflights[index] = Some(result?);
    }

    let mut preflights = Vec::with_capacity(tool_call_count);
    let mut approval_requests = Vec::new();
    for entry in source_order_preflights {
        let (preflight, approval_request) = entry.ok_or_else(|| {
            crate::AgentCoreError::Phase("parallel tool preflight result missing".into())
        })?;
        if let Some(approval_request) = approval_request {
            approval_requests.push(approval_request);
        }
        preflights.push(preflight);
    }
    Ok((preflights, approval_requests))
}

#[derive(Clone)]
struct ToolPreflightContext {
    handles: ToolRuntimeHandles,
    run_id: String,
    turn_id: u64,
    state: AgentState,
    cancellation: CancellationToken,
}

async fn prepare_one_tool_call(
    context: &ToolPreflightContext,
    tool_call: ToolCall,
    start_hook_index: usize,
    permission_audit: Option<ToolPermissionAudit>,
) -> Result<(ToolApprovalPreflight, Option<ToolApprovalRequest>)> {
    context.cancellation.throw_if_cancelled()?;
    let tool = context
        .handles
        .tools
        .get(&tool_call.name)
        .cloned()
        .ok_or_else(|| crate::AgentCoreError::MissingTool(tool_call.name.clone()))?;
    let tool_spec = tool.spec();
    let mut permission_audit = permission_audit.unwrap_or_else(|| ToolPermissionAudit {
        tool_call: tool_call.clone(),
        permissions: tool_spec.permissions.clone(),
        decisions: Vec::new(),
    });
    for (hook_index, hook) in context
        .handles
        .hooks
        .iter()
        .enumerate()
        .skip(start_hook_index)
    {
        let result = hook
            .before_tool_call(
                BeforeToolCallContext {
                    run_id: context.run_id.clone(),
                    turn_id: context.turn_id,
                    tool_call: tool_call.clone(),
                    tool_spec: tool_spec.clone(),
                    state: context.state.clone(),
                },
                context.cancellation.clone(),
            )
            .await?;
        let Some(result) = result else {
            continue;
        };
        match result {
            BeforeToolCallResult::Decision { decision } => {
                permission_audit
                    .decisions
                    .push(ToolPermissionDecisionRecord {
                        hook_id: hook.id().map(ToString::to_string),
                        decision: decision.clone(),
                    });
                if matches!(decision.outcome, ToolPermissionOutcome::Deny) {
                    return Ok((
                        ToolApprovalPreflight {
                            tool_call,
                            permission_audit,
                            status: ToolApprovalPreflightStatus::Denied { decision },
                        },
                        None,
                    ));
                }
            }
            BeforeToolCallResult::Approval { approval: request } => {
                let approval_id =
                    tool_approval_id(&context.run_id, context.turn_id, &tool_call.id, hook_index);
                let approval = ToolApprovalRequest {
                    approval_id: approval_id.clone(),
                    tool_call: tool_call.clone(),
                    permissions: tool_spec.permissions.clone(),
                    hook_id: hook.id().map(ToString::to_string),
                    request,
                };
                return Ok((
                    ToolApprovalPreflight {
                        tool_call,
                        permission_audit,
                        status: ToolApprovalPreflightStatus::Pending {
                            approval_id,
                            hook_index,
                            hook_id: hook.id().map(ToString::to_string),
                        },
                    },
                    Some(approval),
                ));
            }
        }
    }
    Ok((
        ToolApprovalPreflight {
            tool_call,
            permission_audit,
            status: ToolApprovalPreflightStatus::Ready,
        },
        None,
    ))
}

fn tool_approval_id(run_id: &str, turn_id: u64, tool_call_id: &str, hook_index: usize) -> String {
    format!("approval-{run_id}-{turn_id}-{tool_call_id}-{hook_index}")
}

async fn execute_prepared_tools_sequential(
    context: &PhaseContext<'_>,
    preflights: Vec<ToolApprovalPreflight>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let mut source_order_outputs = Vec::new();
    let handles = context.runtime.tool_handles();
    for preflight in preflights {
        let tool_call = preflight.tool_call.clone();
        let execution = execute_prepared_tool_call(
            handles.clone(),
            context.run_id.to_string(),
            context.turn_id,
            context.state.clone(),
            preflight,
            context.cancellation.clone(),
        )
        .await?;
        output
            .completed_tool_outputs
            .push((tool_call.clone(), execution.output.clone()));
        output
            .completed_tool_permission_audits
            .push(execution.permission_audit.clone());
        source_order_outputs.push((tool_call, execution.output));
    }
    Ok(source_order_outputs)
}

async fn execute_prepared_tools_parallel(
    context: &PhaseContext<'_>,
    preflights: Vec<ToolApprovalPreflight>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let (sender, mut receiver) = mpsc::channel(preflights.len());
    let handles = context.runtime.tool_handles();
    let preflight_count = preflights.len();
    for (index, preflight) in preflights.into_iter().enumerate() {
        let sender = sender.clone();
        let run_id = context.run_id.to_string();
        let handles = handles.clone();
        let state = context.state.clone();
        let cancellation = context.cancellation.clone();
        let turn_id = context.turn_id;
        let tool_call = preflight.tool_call.clone();
        tokio::spawn(async move {
            let result = execute_prepared_tool_call(
                handles,
                run_id,
                turn_id,
                state,
                preflight,
                cancellation,
            )
            .await;
            let _ = sender.send((index, tool_call, result)).await;
        });
    }
    drop(sender);

    let mut source_order_outputs = vec![None; preflight_count];
    while let Some((index, tool_call, result)) = receiver.recv().await {
        let execution = result?;
        output
            .completed_tool_outputs
            .push((tool_call.clone(), execution.output.clone()));
        output
            .completed_tool_permission_audits
            .push(execution.permission_audit.clone());
        source_order_outputs[index] = Some((tool_call, execution.output));
    }

    source_order_outputs
        .into_iter()
        .map(|entry| {
            entry.ok_or_else(|| crate::AgentCoreError::Phase("parallel tool result missing".into()))
        })
        .collect()
}

async fn execute_prepared_tool_call(
    handles: ToolRuntimeHandles,
    run_id: String,
    turn_id: u64,
    state: AgentState,
    preflight: ToolApprovalPreflight,
    cancellation: CancellationToken,
) -> Result<ToolExecutionOutcome> {
    cancellation.throw_if_cancelled()?;
    let tool_call = preflight.tool_call.clone();
    let permission_audit = preflight.permission_audit;
    if let ToolApprovalPreflightStatus::Denied { decision } = preflight.status {
        return Ok(ToolExecutionOutcome {
            output: denied_tool_output(&decision),
            permission_audit,
        });
    }
    if matches!(
        preflight.status,
        ToolApprovalPreflightStatus::Pending { .. }
    ) {
        return Err(crate::AgentCoreError::Phase(
            "cannot execute tool with pending approval".into(),
        ));
    }
    let tool = handles
        .tools
        .get(&tool_call.name)
        .cloned()
        .ok_or_else(|| crate::AgentCoreError::MissingTool(tool_call.name.clone()))?;

    let request = crate::ToolRequest {
        run_id: run_id.clone(),
        turn_id,
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        state: state.clone(),
    };
    let mut output = match tool.execute_tool(request, cancellation.clone()).await {
        Ok(output) => output,
        Err(crate::AgentCoreError::Aborted) => return Err(crate::AgentCoreError::Aborted),
        Err(error) => error_tool_output(error.to_string()),
    };

    for hook in &handles.hooks {
        if let Some(rewrite) = hook
            .after_tool_call(
                AfterToolCallContext {
                    run_id: run_id.clone(),
                    turn_id,
                    tool_call: tool_call.clone(),
                    output: output.clone(),
                    state: state.clone(),
                },
                cancellation.clone(),
            )
            .await?
        {
            if let Some(content) = rewrite.content {
                output.content = content;
            }
            if let Some(details) = rewrite.details {
                output.details = details;
            }
            if let Some(is_error) = rewrite.is_error {
                output.is_error = is_error;
            }
        }
    }

    Ok(ToolExecutionOutcome {
        output,
        permission_audit,
    })
}

struct ToolExecutionOutcome {
    output: ToolOutput,
    permission_audit: ToolPermissionAudit,
}

fn denied_tool_output(decision: &ToolPermissionDecision) -> ToolOutput {
    let mut output = error_tool_output(
        decision
            .reason
            .clone()
            .unwrap_or_else(|| "tool execution was denied".into()),
    );
    output.details = json!({ "permissionDecision": decision });
    output
}

fn error_tool_output(message: String) -> ToolOutput {
    ToolOutput {
        content: vec![ContentBlock::Text { text: message }],
        details: json!({}),
        is_error: true,
        updates: Vec::new(),
    }
}

async fn turn_decision(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    output.scratch.decision = Some(
        if output.scratch.tool_calls.is_empty() || context.turn_id >= context.runtime.max_turns() {
            TurnDecision::Stop
        } else {
            TurnDecision::Continue
        },
    );
    Ok(output)
}
