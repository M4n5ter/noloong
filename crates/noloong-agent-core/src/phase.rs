use crate::runtime::ToolRuntimeHandles;
use crate::{
    AfterToolCallContext, AgentEffect, AgentMessage, BeforeToolCallContext, ContentBlock,
    ContextRequest, MediaBlock, MediaDelta, MediaSource, ModelRequest, ModelStreamEvent, Result,
    ThinkingBlock, ToolCall, ToolExecutionMode, ToolOutput, TurnDecision,
    providers::{BoxFuture, CancellationToken, ModelStreamSink},
};
use crate::{AgentRuntime, AgentState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub const PHASE_INPUT_INGEST: &str = "input.ingest";
pub const PHASE_CONTEXT_PREPARE: &str = "context.prepare";
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseScratch {
    #[serde(default)]
    pub input: Option<AgentMessage>,
    #[serde(default)]
    pub model_request: Option<ModelRequest>,
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
        }
    }
}

#[derive(Clone, Debug)]
pub enum StandardPhase {
    InputIngest,
    ContextPrepare,
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

async fn model_request_prepare(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    let context_map = context
        .state
        .context
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    output.scratch.model_request = Some(ModelRequest {
        run_id: context.run_id.to_string(),
        turn_id: context.turn_id,
        messages: context.state.messages.clone(),
        context: context_map,
        tools: context
            .runtime
            .tool_specs()
            .into_iter()
            .map(|tool| tool.spec())
            .collect(),
        metadata: Default::default(),
    });
    Ok(output)
}

async fn model_stream(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
    context.cancellation.throw_if_cancelled()?;
    let provider = context.runtime.default_model_provider()?;
    let request = output
        .scratch
        .model_request
        .clone()
        .ok_or_else(|| crate::AgentCoreError::Phase("model request was not prepared".into()))?;
    let emitted_events = Arc::new(Mutex::new(Vec::new()));
    let outer_sink = context.model_stream_sink.clone();
    let emitted_events_for_sink = Arc::clone(&emitted_events);
    let sink: ModelStreamSink = Arc::new(move |event| {
        let emitted_events = Arc::clone(&emitted_events_for_sink);
        let outer_sink = outer_sink.clone();
        Box::pin(async move {
            emitted_events.lock().await.push(event.clone());
            if let Some(outer_sink) = outer_sink {
                outer_sink(event).await?;
            }
            Ok(())
        })
    });
    let returned_events = provider
        .stream_model(request, sink, context.cancellation.clone())
        .await?;
    let emitted_events = emitted_events.lock().await.clone();
    let events = if emitted_events.is_empty() {
        output.stream_events = returned_events.clone();
        returned_events
    } else {
        emitted_events
    };
    output.scratch.model_events = events;
    Ok(output)
}

async fn assistant_commit(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch);
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

    let should_run_sequential = context.runtime.tool_execution_mode()
        == ToolExecutionMode::Sequential
        || tool_calls.iter().any(|tool_call| {
            context
                .runtime
                .tool(&tool_call.name)
                .ok()
                .and_then(|tool| tool.spec().execution_mode)
                == Some(ToolExecutionMode::Sequential)
        });

    let source_order_outputs = if should_run_sequential {
        execute_tools_sequential(&context, tool_calls, &mut output).await?
    } else {
        execute_tools_parallel(&context, tool_calls, &mut output).await?
    };

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
                format!("tool-result-{}-{}", context.run_id, tool_call.id),
                tool_call.id,
                tool_call.name,
                tool_output,
            ),
        });
    }
    Ok(output)
}

async fn execute_tools_sequential(
    context: &PhaseContext<'_>,
    tool_calls: Vec<ToolCall>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let mut source_order_outputs = Vec::new();
    let handles = context.runtime.tool_handles();
    for tool_call in tool_calls {
        let tool_output = execute_one_tool_call(
            handles.clone(),
            context.run_id.to_string(),
            context.turn_id,
            context.state.clone(),
            tool_call.clone(),
            context.cancellation.clone(),
        )
        .await?;
        output
            .completed_tool_outputs
            .push((tool_call.clone(), tool_output.clone()));
        source_order_outputs.push((tool_call, tool_output));
    }
    Ok(source_order_outputs)
}

async fn execute_tools_parallel(
    context: &PhaseContext<'_>,
    tool_calls: Vec<ToolCall>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let (sender, mut receiver) = mpsc::channel(tool_calls.len());
    let handles = context.runtime.tool_handles();
    for (index, tool_call) in tool_calls.iter().cloned().enumerate() {
        let sender = sender.clone();
        let run_id = context.run_id.to_string();
        let handles = handles.clone();
        let state = context.state.clone();
        let cancellation = context.cancellation.clone();
        let turn_id = context.turn_id;
        tokio::spawn(async move {
            let result = execute_one_tool_call(
                handles,
                run_id,
                turn_id,
                state,
                tool_call.clone(),
                cancellation,
            )
            .await;
            let _ = sender.send((index, tool_call, result)).await;
        });
    }
    drop(sender);

    let mut source_order_outputs = vec![None; tool_calls.len()];
    while let Some((index, tool_call, result)) = receiver.recv().await {
        let tool_output = result?;
        output
            .completed_tool_outputs
            .push((tool_call.clone(), tool_output.clone()));
        source_order_outputs[index] = Some((tool_call, tool_output));
    }

    source_order_outputs
        .into_iter()
        .map(|entry| {
            entry.ok_or_else(|| crate::AgentCoreError::Phase("parallel tool result missing".into()))
        })
        .collect()
}

async fn execute_one_tool_call(
    handles: ToolRuntimeHandles,
    run_id: String,
    turn_id: u64,
    state: AgentState,
    tool_call: ToolCall,
    cancellation: CancellationToken,
) -> Result<ToolOutput> {
    cancellation.throw_if_cancelled()?;
    for hook in &handles.hooks {
        let result = hook
            .before_tool_call(
                BeforeToolCallContext {
                    run_id: run_id.clone(),
                    turn_id,
                    tool_call: tool_call.clone(),
                    state: state.clone(),
                },
                cancellation.clone(),
            )
            .await?;
        if let Some(result) = result
            && result.block
        {
            return Ok(error_tool_output(
                result
                    .reason
                    .unwrap_or_else(|| "tool execution was blocked".into()),
            ));
        }
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

    Ok(output)
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
