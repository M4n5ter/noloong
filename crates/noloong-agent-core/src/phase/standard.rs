use super::{PhaseContext, PhaseOutput};
use crate::compaction::{
    COMPACTION_METADATA_IS_SPLIT_TURN_KEY, COMPACTION_METADATA_MODE_KEY,
    COMPACTION_METADATA_TOKENS_BEFORE_KEY,
};
use crate::{
    AgentEffect, CompactionDecision, ContentBlock, ContextCompactionMode, ContextRequest,
    MessageCompaction, ModelRequest, Result, TurnDecision, compacted_messages,
    compaction_summary_message, plan_compaction, provider_utils::collect_model_stream,
};

use super::hooks::PhaseHookRunner;

pub(super) async fn input_ingest(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn context_prepare(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn context_compact(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn model_request_prepare(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn model_stream(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn tool_call_resolve(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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

pub(super) async fn turn_decision(context: PhaseContext<'_>) -> Result<PhaseOutput> {
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
