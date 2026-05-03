use crate::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, AfterToolCallContext, AfterToolCallResult, AgentCoreError,
    AgentEffect, AgentMessage, BeforeAssistantCommitHookContext, BeforeAssistantCommitHookResult,
    BeforeModelRequestHookContext, BeforeModelRequestHookResult, BeforeToolCallContext,
    BeforeToolCallResult, CompactionSummarizer, CompactionSummaryRequest, CompactionSummaryResult,
    ContentBlock, ContextProvider, ContextRequest, ExtensionCapability, ExtensionManifest,
    ModelProvider, ModelRequest, ModelStreamEvent, PhaseContext, PhaseHook, PhaseNode, PhaseOutput,
    Result, ToolCallHook, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use crate::{CancellationToken, ModelStreamSink};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
    time::timeout,
};

#[derive(Clone, Debug)]
pub struct StdioExtensionConfig {
    pub command: String,
    pub args: Vec<String>,
    pub request_timeout: Duration,
    pub stream_timeout: Duration,
}

impl StdioExtensionConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            request_timeout: Duration::from_secs(5),
            stream_timeout: Duration::from_secs(30),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    pub fn stream_timeout(mut self, stream_timeout: Duration) -> Self {
        self.stream_timeout = stream_timeout;
        self
    }
}

pub struct StdioExtension {
    manifest: ExtensionManifest,
    writer: Arc<Mutex<ChildStdin>>,
    pending: PendingRequests,
    model_stream_sinks: ModelStreamRegistrations,
    request_counter: AtomicU64,
    request_timeout: Duration,
    stream_timeout: Duration,
    _child: Mutex<Child>,
}

type PendingRequests = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>;
type ModelStreamRegistrations = Arc<Mutex<BTreeMap<String, ModelStreamRegistration>>>;

#[derive(Clone)]
struct ModelStreamRegistration {
    sink: ModelStreamSink,
    events: mpsc::UnboundedSender<Result<ModelStreamEvent>>,
}

impl StdioExtension {
    pub async fn connect(config: StdioExtensionConfig) -> Result<Self> {
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentCoreError::JsonRpc("extension stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentCoreError::JsonRpc("extension stdout unavailable".into()))?;

        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let model_stream_sinks = Arc::new(Mutex::new(BTreeMap::new()));
        tokio::spawn(read_stdout(
            stdout,
            pending.clone(),
            model_stream_sinks.clone(),
        ));

        let extension = Self {
            manifest: ExtensionManifest {
                name: config.command.clone(),
                version: "unknown".into(),
            },
            writer: Arc::new(Mutex::new(stdin)),
            pending,
            model_stream_sinks,
            request_counter: AtomicU64::new(0),
            request_timeout: config.request_timeout,
            stream_timeout: config.stream_timeout,
            _child: Mutex::new(child),
        };

        let manifest = extension
            .request::<InitializeResult>("initialize", json!({ "protocolVersion": 1 }), None)
            .await?
            .manifest;

        Ok(Self {
            manifest,
            ..extension
        })
    }

    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    pub async fn capabilities(&self) -> Result<Vec<ExtensionCapability>> {
        Ok(self
            .request::<CapabilitiesResult>("capabilities/list", json!({}), None)
            .await?
            .capabilities)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.request::<Value>("shutdown", json!({}), None).await?;
        Ok(())
    }

    async fn request<T>(
        &self,
        method: &str,
        params: Value,
        cancellation: Option<CancellationToken>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let id = self.request_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let payload = serde_json::to_vec(&request)?;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let write_result = {
            let mut writer = self.writer.lock().await;
            async {
                writer.write_all(&payload).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await
            }
            .await
        };
        if let Err(error) = write_result {
            self.pending.lock().await.remove(&id);
            return Err(error.into());
        }

        let response = if let Some(cancellation) = cancellation {
            tokio::select! {
                response = timeout(self.request_timeout, receiver) => {
                    match response {
                        Ok(Ok(response)) => response,
                        Ok(Err(_)) => {
                            self.pending.lock().await.remove(&id);
                            return Err(AgentCoreError::JsonRpc(format!("response channel closed: {method}")));
                        }
                        Err(_) => {
                            self.pending.lock().await.remove(&id);
                            return Err(AgentCoreError::JsonRpc(format!("request timed out: {method}")));
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::Aborted);
                }
            }
        } else {
            match timeout(self.request_timeout, receiver).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::JsonRpc(format!(
                        "response channel closed: {method}"
                    )));
                }
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::JsonRpc(format!(
                        "request timed out: {method}"
                    )));
                }
            }
        }?;
        Ok(serde_json::from_value(response)?)
    }

    async fn register_model_stream(
        &self,
        stream_id: String,
        sink: ModelStreamSink,
    ) -> mpsc::UnboundedReceiver<Result<ModelStreamEvent>> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.model_stream_sinks.lock().await.insert(
            stream_id,
            ModelStreamRegistration {
                sink,
                events: sender,
            },
        );
        receiver
    }

    async fn unregister_model_stream(&self, stream_id: &str) {
        self.model_stream_sinks.lock().await.remove(stream_id);
    }
}

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
            let stream_id = format!(
                "model-{}",
                self.extension.request_counter.load(Ordering::SeqCst) + 1
            );
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
    stream_events: &mut mpsc::UnboundedReceiver<Result<ModelStreamEvent>>,
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
            Ok(output
                .decision
                .map(|decision| BeforeToolCallResult { decision }))
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

async fn read_stdout(
    stdout: tokio::process::ChildStdout,
    pending: PendingRequests,
    model_stream_sinks: ModelStreamRegistrations,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(value) => handle_message(value, &pending, &model_stream_sinks).await,
            Err(error) => {
                let mut pending = pending.lock().await;
                let pending = std::mem::take(&mut *pending);
                for (_, sender) in pending {
                    let _ = sender.send(Err(AgentCoreError::JsonRpc(format!(
                        "invalid json from extension: {error}"
                    ))));
                }
            }
        }
    }

    let mut pending = pending.lock().await;
    let pending = std::mem::take(&mut *pending);
    for (_, sender) in pending {
        let _ = sender.send(Err(AgentCoreError::JsonRpc(
            "extension stdout closed".into(),
        )));
    }
}

async fn handle_message(
    value: Value,
    pending: &PendingRequests,
    model_stream_sinks: &ModelStreamRegistrations,
) {
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        let sender = pending.lock().await.remove(&id);
        if let Some(sender) = sender {
            let result = if let Some(error) = value.get("error") {
                Err(AgentCoreError::JsonRpc(error.to_string()))
            } else {
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(result);
        }
        return;
    }

    if value.get("method").and_then(Value::as_str) != Some("stream/event") {
        return;
    }
    let Some(params) = value.get("params") else {
        return;
    };
    let Some(stream_id) = params.get("streamId").and_then(Value::as_str) else {
        return;
    };
    let Some(event) = params.get("event") else {
        return;
    };
    if let Some(registration) = model_stream_sinks.lock().await.get(stream_id).cloned() {
        match serde_json::from_value::<ModelStreamEvent>(event.clone()) {
            Ok(event) => {
                let result = (registration.sink)(event.clone()).await.map(|()| event);
                let _ = registration.events.send(result);
            }
            Err(error) => {
                let _ = registration
                    .events
                    .send(Err(AgentCoreError::JsonRpc(format!(
                        "invalid stream event for {stream_id}: {error}"
                    ))));
            }
        }
    }
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResult {
    manifest: ExtensionManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesResult {
    capabilities: Vec<ExtensionCapability>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamResult {
    stream_id: Option<String>,
    #[serde(default)]
    events: Vec<ModelStreamEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextResult {
    #[serde(default)]
    effects: Vec<AgentEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhaseHookOutput {
    #[serde(default)]
    model_request: Option<ModelRequest>,
    #[serde(default)]
    model_events: Option<Vec<ModelStreamEvent>>,
    #[serde(default)]
    assistant_message: Option<AgentMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeforeToolHookOutput {
    #[serde(default)]
    decision: Option<crate::ToolPermissionDecision>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AfterToolHookOutput {
    #[serde(default)]
    content: Option<Vec<ContentBlock>>,
    #[serde(default)]
    details: Option<Value>,
    #[serde(default)]
    is_error: Option<bool>,
}
