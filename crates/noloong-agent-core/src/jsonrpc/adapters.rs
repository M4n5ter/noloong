use super::{StdioExtension, wire};
use crate::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, AfterToolCallContext, AfterToolCallResult, AgentCoreError,
    AgentEffect, AgentMessage, BeforeAssistantCommitHookContext, BeforeAssistantCommitHookResult,
    BeforeModelRequestHookContext, BeforeModelRequestHookResult, BeforeToolCallContext,
    BeforeToolCallResult, CompactionSummarizer, CompactionSummaryRequest, CompactionSummaryResult,
    ContextCompactionOutput, ContextCompactionRequest, ContextCompactor, ContextProvider,
    ContextRequest, HttpAuthContext, HttpAuthHeaders, HttpAuthProvider, HttpAuthRefreshContext,
    HttpAuthRefreshReason, HttpAuthRefreshResult, ModelProvider, ModelRequest, ModelStreamEvent,
    PhaseContext, PhaseHook, PhaseNode, PhaseOutput, Result, ToolCallHook, ToolOutput,
    ToolProvider, ToolRequest, ToolSpec,
};
use crate::{CancellationToken, ModelStreamSink};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

use wire::{
    AfterToolHookOutput, BeforeToolHookOutput, ContextResult, PhaseHookOutput, StreamResult,
};

pub struct StdioModelProvider {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioModelProvider {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl ModelProvider for StdioModelProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let stream_id = format!("model-{}", self.extension.next_request_id());
            let mut stream_events = self
                .extension
                .register_model_stream(stream_id.clone(), stream.clone())
                .await;

            let extension = Arc::clone(&self.extension);
            let provider_id = self.id.clone();
            let request_stream_id = stream_id.clone();
            let request_cancellation = cancellation.clone();
            let response_task = tokio::spawn(async move {
                extension
                    .request::<StreamResult>(
                        "model/stream",
                        json!({
                            "providerId": provider_id,
                            "streamId": request_stream_id,
                            "request": request,
                        }),
                        Some(request_cancellation),
                    )
                    .await
            });

            let result = collect_model_stream(
                &self.extension,
                &stream_id,
                stream,
                &mut stream_events,
                response_task,
                cancellation,
            )
            .await;
            self.extension.unregister_model_stream(&stream_id).await;
            result
        })
    }
}

async fn collect_model_stream(
    extension: &StdioExtension,
    stream_id: &str,
    stream: ModelStreamSink,
    stream_events: &mut mpsc::Receiver<Result<ModelStreamEvent>>,
    mut response_task: tokio::task::JoinHandle<Result<StreamResult>>,
    cancellation: CancellationToken,
) -> Result<Vec<ModelStreamEvent>> {
    let mut events = Vec::new();
    let mut response_done = false;
    let stream_timeout = tokio::time::sleep(extension.stream_timeout);
    tokio::pin!(stream_timeout);

    loop {
        tokio::select! {
            maybe_event = stream_events.recv() => {
                let Some(event) = maybe_event else {
                    if response_done {
                        return Ok(events);
                    }
                    continue;
                };
                let event = event?;
                let terminal = model_stream_event_is_terminal(&event);
                events.push(event);
                if terminal {
                    return Ok(events);
                }
            }
            response = &mut response_task, if !response_done => {
                response_done = true;
                let response = response
                    .map_err(|error| AgentCoreError::JsonRpc(format!("model stream task failed: {error}")))??;
                if response
                    .stream_id
                    .as_deref()
                    .is_some_and(|response_stream_id| response_stream_id != stream_id)
                {
                    return Err(AgentCoreError::JsonRpc(format!(
                        "model stream id mismatch: expected {stream_id}"
                    )));
                }
                for event in response.events {
                    stream(event.clone()).await?;
                    let terminal = model_stream_event_is_terminal(&event);
                    events.push(event);
                    if terminal {
                        return Ok(events);
                    }
                }
                if !events.is_empty() {
                    return Ok(events);
                }
            }
            _ = &mut stream_timeout => {
                return Err(AgentCoreError::JsonRpc(format!("model stream timed out: {stream_id}")));
            }
            _ = cancellation.cancelled() => {
                return Err(AgentCoreError::Aborted);
            }
        }
    }
}

fn model_stream_event_is_terminal(event: &ModelStreamEvent) -> bool {
    matches!(
        event,
        ModelStreamEvent::Finished { .. } | ModelStreamEvent::Failed { .. }
    )
}

pub struct StdioToolProvider {
    extension: Arc<StdioExtension>,
    spec: ToolSpec,
}

impl StdioToolProvider {
    pub fn new(extension: Arc<StdioExtension>, spec: ToolSpec) -> Self {
        Self { extension, spec }
    }
}

impl ToolProvider for StdioToolProvider {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            self.extension
                .request::<ToolOutput>(
                    "tool/execute",
                    json!({
                        "toolName": self.spec.name,
                        "request": request,
                    }),
                    Some(cancellation),
                )
                .await
        })
    }
}

pub struct StdioToolCallHook {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioToolCallHook {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl ToolCallHook for StdioToolCallHook {
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async move {
            let output: BeforeToolHookOutput = self
                .run_tool_hook(
                    ToolHookPoint::BeforeToolCall,
                    context.run_id,
                    context.turn_id,
                    &context.state,
                    BeforeToolCallHookPayload {
                        tool_call: &context.tool_call,
                        tool_spec: &context.tool_spec,
                        permissions: &context.tool_spec.permissions,
                    },
                    cancellation,
                )
                .await?;
            match (output.decision, output.approval) {
                (Some(_), Some(_)) => Err(crate::AgentCoreError::JsonRpc(
                    "before_tool_call returned both decision and approval".into(),
                )),
                (None, None) => Ok(None),
                (Some(decision), None) => Ok(Some(BeforeToolCallResult::decision(decision))),
                (None, Some(approval)) => Ok(Some(BeforeToolCallResult::approval(approval))),
            }
        })
    }

    fn after_tool_call<'a>(
        &'a self,
        context: AfterToolCallContext,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<AfterToolCallResult>> {
        Box::pin(async move {
            let output: AfterToolHookOutput = self
                .run_tool_hook(
                    ToolHookPoint::AfterToolCall,
                    context.run_id,
                    context.turn_id,
                    &context.state,
                    AfterToolCallHookPayload {
                        tool_call: &context.tool_call,
                        output: &context.output,
                    },
                    cancellation,
                )
                .await?;
            if output.content.is_none() && output.details.is_none() && output.is_error.is_none() {
                return Ok(None);
            }
            Ok(Some(AfterToolCallResult {
                content: output.content,
                details: output.details,
                is_error: output.is_error,
            }))
        })
    }
}

impl StdioToolCallHook {
    async fn run_tool_hook<P, O>(
        &self,
        hook_point: ToolHookPoint,
        run_id: String,
        turn_id: u64,
        state: &crate::AgentState,
        payload: P,
        cancellation: CancellationToken,
    ) -> Result<O>
    where
        P: Serialize,
        O: DeserializeOwned,
    {
        cancellation.throw_if_cancelled()?;
        let params = serde_json::to_value(HookRequest {
            hook_id: &self.id,
            hook_point: hook_point.as_str(),
            run_id: &run_id,
            turn_id,
            state,
            payload,
        })?;
        self.extension
            .request("tool_hook/run", params, Some(cancellation))
            .await
    }
}

pub struct StdioContextProvider {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioContextProvider {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl ContextProvider for StdioContextProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare_context<'a>(
        &'a self,
        request: ContextRequest,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Vec<AgentEffect>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(self
                .extension
                .request::<ContextResult>(
                    "context/apply",
                    json!({
                        "providerId": self.id,
                        "request": request,
                    }),
                    Some(cancellation),
                )
                .await?
                .effects)
        })
    }
}

pub struct StdioCompactionSummarizer {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioCompactionSummarizer {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl CompactionSummarizer for StdioCompactionSummarizer {
    fn id(&self) -> &str {
        &self.id
    }

    fn summarize<'a>(
        &'a self,
        request: CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let params = serde_json::to_value(CompactionSummarizeRequest {
                summarizer_id: &self.id,
                request: &request,
            })?;
            self.extension
                .request("compaction/summarize", params, Some(cancellation))
                .await
        })
    }
}

pub struct StdioContextCompactor {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioContextCompactor {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl ContextCompactor for StdioContextCompactor {
    fn id(&self) -> &str {
        &self.id
    }

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, ContextCompactionOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let params = serde_json::to_value(ContextCompactionRequestPayload {
                compactor_id: &self.id,
                request: &request,
            })?;
            self.extension
                .request("compaction/compact", params, Some(cancellation))
                .await
        })
    }
}

pub struct StdioHttpAuthProvider {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioHttpAuthProvider {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl HttpAuthProvider for StdioHttpAuthProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn headers<'a>(
        &'a self,
        context: HttpAuthContext,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, HttpAuthHeaders> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let params = serde_json::to_value(HttpAuthHeadersRequest {
                auth_provider_id: &self.id,
                context: &context,
            })?;
            self.extension
                .request("auth/headers", params, Some(cancellation))
                .await
        })
    }

    fn refresh<'a>(
        &'a self,
        context: HttpAuthRefreshContext,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, HttpAuthRefreshResult> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let params = serde_json::to_value(HttpAuthRefreshRequest {
                auth_provider_id: &self.id,
                context: &context.context,
                reason: &context.reason,
                status: context.status,
                metadata: &context.metadata,
            })?;
            self.extension
                .request("auth/refresh", params, Some(cancellation))
                .await
        })
    }
}

pub struct StdioPhaseNode {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioPhaseNode {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl PhaseNode for StdioPhaseNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn run<'a>(
        &'a self,
        context: PhaseContext<'a>,
    ) -> crate::providers::BoxFuture<'a, PhaseOutput> {
        Box::pin(async move {
            self.extension
                .request::<PhaseOutput>(
                    "phase/run",
                    json!({
                        "phaseId": self.id,
                        "request": {
                            "runId": context.run_id,
                            "turnId": context.turn_id,
                            "state": context.state,
                            "scratch": context.scratch,
                        },
                    }),
                    Some(context.cancellation),
                )
                .await
        })
    }
}

pub struct StdioPhaseHook {
    extension: Arc<StdioExtension>,
    id: String,
}

impl StdioPhaseHook {
    pub fn new(extension: Arc<StdioExtension>, id: String) -> Self {
        Self { extension, id }
    }
}

impl PhaseHook for StdioPhaseHook {
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn before_model_request<'a>(
        &'a self,
        context: BeforeModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async move {
            Ok(self
                .run_phase_hook(
                    PhaseHookPoint::BeforeModelRequest,
                    context.run_id,
                    context.turn_id,
                    context.state,
                    BeforeModelRequestHookPayload {
                        model_request: context.request,
                    },
                    cancellation,
                )
                .await?
                .model_request
                .map(|request| BeforeModelRequestHookResult { request }))
        })
    }

    fn after_model_request<'a>(
        &'a self,
        context: AfterModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<AfterModelRequestHookResult>> {
        Box::pin(async move {
            Ok(self
                .run_phase_hook(
                    PhaseHookPoint::AfterModelRequest,
                    context.run_id,
                    context.turn_id,
                    context.state,
                    AfterModelRequestHookPayload {
                        model_request: context.request,
                        model_events: context.events,
                    },
                    cancellation,
                )
                .await?
                .model_events
                .map(|events| AfterModelRequestHookResult { events }))
        })
    }

    fn before_assistant_commit<'a>(
        &'a self,
        context: BeforeAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<BeforeAssistantCommitHookResult>> {
        Box::pin(async move {
            Ok(self
                .run_phase_hook(
                    PhaseHookPoint::BeforeAssistantCommit,
                    context.run_id,
                    context.turn_id,
                    context.state,
                    BeforeAssistantCommitHookPayload {
                        model_events: context.events,
                    },
                    cancellation,
                )
                .await?
                .model_events
                .map(|events| BeforeAssistantCommitHookResult { events }))
        })
    }

    fn after_assistant_commit<'a>(
        &'a self,
        context: AfterAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Option<AfterAssistantCommitHookResult>> {
        Box::pin(async move {
            Ok(self
                .run_phase_hook(
                    PhaseHookPoint::AfterAssistantCommit,
                    context.run_id,
                    context.turn_id,
                    context.state,
                    AfterAssistantCommitHookPayload {
                        assistant_message: context.message,
                    },
                    cancellation,
                )
                .await?
                .assistant_message
                .map(|message| AfterAssistantCommitHookResult { message }))
        })
    }
}

impl StdioPhaseHook {
    async fn run_phase_hook<P>(
        &self,
        hook_point: PhaseHookPoint,
        run_id: &str,
        turn_id: u64,
        state: &crate::AgentState,
        payload: P,
        cancellation: CancellationToken,
    ) -> Result<PhaseHookOutput>
    where
        P: Serialize,
    {
        cancellation.throw_if_cancelled()?;
        let params = serde_json::to_value(HookRequest {
            hook_id: &self.id,
            hook_point: hook_point.as_str(),
            run_id,
            turn_id,
            state,
            payload,
        })?;
        self.extension
            .request("phase_hook/run", params, Some(cancellation))
            .await
    }
}

#[derive(Clone, Copy)]
enum PhaseHookPoint {
    BeforeModelRequest,
    AfterModelRequest,
    BeforeAssistantCommit,
    AfterAssistantCommit,
}

impl PhaseHookPoint {
    fn as_str(self) -> &'static str {
        match self {
            Self::BeforeModelRequest => "before_model_request",
            Self::AfterModelRequest => "after_model_request",
            Self::BeforeAssistantCommit => "before_assistant_commit",
            Self::AfterAssistantCommit => "after_assistant_commit",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HookRequest<'a, P> {
    hook_id: &'a str,
    hook_point: &'static str,
    run_id: &'a str,
    turn_id: u64,
    state: &'a crate::AgentState,
    #[serde(flatten)]
    payload: P,
}

#[derive(Clone, Copy)]
enum ToolHookPoint {
    BeforeToolCall,
    AfterToolCall,
}

impl ToolHookPoint {
    fn as_str(self) -> &'static str {
        match self {
            Self::BeforeToolCall => "before_tool_call",
            Self::AfterToolCall => "after_tool_call",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeToolCallHookPayload<'a> {
    tool_call: &'a crate::ToolCall,
    tool_spec: &'a ToolSpec,
    permissions: &'a [crate::ToolPermissionRequirement],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AfterToolCallHookPayload<'a> {
    tool_call: &'a crate::ToolCall,
    output: &'a ToolOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeModelRequestHookPayload<'a> {
    model_request: &'a ModelRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AfterModelRequestHookPayload<'a> {
    model_request: &'a ModelRequest,
    model_events: &'a [ModelStreamEvent],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeAssistantCommitHookPayload<'a> {
    model_events: &'a [ModelStreamEvent],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AfterAssistantCommitHookPayload<'a> {
    assistant_message: &'a AgentMessage,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompactionSummarizeRequest<'a> {
    summarizer_id: &'a str,
    #[serde(flatten)]
    request: &'a CompactionSummaryRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextCompactionRequestPayload<'a> {
    compactor_id: &'a str,
    #[serde(flatten)]
    request: &'a ContextCompactionRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpAuthHeadersRequest<'a> {
    auth_provider_id: &'a str,
    context: &'a HttpAuthContext,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpAuthRefreshRequest<'a> {
    auth_provider_id: &'a str,
    context: &'a HttpAuthContext,
    reason: &'a HttpAuthRefreshReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    metadata: &'a serde_json::Map<String, serde_json::Value>,
}
