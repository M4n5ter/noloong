use crate::phase::{
    PHASE_ASSISTANT_COMMIT, PHASE_CONTEXT_COMPACT, PHASE_CONTEXT_PREPARE, PHASE_INPUT_INGEST,
    PHASE_MODEL_REQUEST_PREPARE, PHASE_MODEL_STREAM, PHASE_TOOL_CALL_RESOLVE, PHASE_TOOL_EXECUTE,
    PHASE_TURN_DECISION,
};
use crate::reducer::{apply_event, reduce_events, validate_effect_for_state};
use crate::{
    AgentCoreError, AgentEffect, AgentEvent, AgentEventKind, AgentMessage, AgentState,
    CompactionSummarizer, ContextCompactionConfig, EventSinkFuture, EventStore,
    HeuristicTokenEstimator, InMemoryEventStore, ModelProvider, ModelStreamEvent, PhaseContext,
    PhaseHook, PhaseNode, PhaseScratch, Result, StdioExtension, StdioExtensionConfig,
    TokenEstimator, ToolCallHook, ToolExecutionMode, ToolProvider, TurnDecision,
};
use crate::{CancellationToken, ContextProvider, ModelStreamSink, StandardPhase};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub type AgentEventSink = Arc<dyn Fn(AgentEvent) -> EventSinkFuture + Send + Sync>;

pub trait RuntimeQueues: Send + Sync {
    fn steering_messages<'a>(&'a self) -> crate::providers::BoxFuture<'a, Vec<AgentMessage>>;
    fn follow_up_messages<'a>(&'a self) -> crate::providers::BoxFuture<'a, Vec<AgentMessage>>;
}

#[derive(Clone)]
pub(crate) struct ToolRuntimeHandles {
    pub tools: BTreeMap<String, Arc<dyn ToolProvider>>,
    pub hooks: Vec<Arc<dyn ToolCallHook>>,
}

#[derive(Clone)]
pub(crate) struct ContextCompactionRuntime {
    pub config: ContextCompactionConfig,
    pub summarizer: Arc<dyn CompactionSummarizer>,
    pub estimator: Arc<dyn TokenEstimator>,
}

enum ContextCompactionRegistration {
    Direct {
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
        estimator: Arc<dyn TokenEstimator>,
    },
    SummarizerId {
        config: ContextCompactionConfig,
        summarizer_id: String,
        estimator: Arc<dyn TokenEstimator>,
    },
}

pub enum AgentInput {
    Text(String),
    Message(AgentMessage),
}

impl From<&str> for AgentInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for AgentInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<AgentMessage> for AgentInput {
    fn from(value: AgentMessage) -> Self {
        Self::Message(value)
    }
}

#[derive(Clone, Debug)]
pub struct RunReport {
    pub run_id: String,
    pub events: Vec<AgentEvent>,
    pub state: AgentState,
}

pub struct AgentRuntime {
    event_store: Arc<dyn EventStore>,
    phases: Vec<Arc<dyn PhaseNode>>,
    model_providers: BTreeMap<String, Arc<dyn ModelProvider>>,
    default_model_provider: String,
    tools: BTreeMap<String, Arc<dyn ToolProvider>>,
    tool_execution_mode: ToolExecutionMode,
    tool_hooks: Vec<Arc<dyn ToolCallHook>>,
    phase_hooks: Vec<Arc<dyn PhaseHook>>,
    context_providers: Vec<Arc<dyn ContextProvider>>,
    context_compaction: Option<ContextCompactionRuntime>,
    _stdio_extensions: Vec<Arc<StdioExtension>>,
    max_turns: u64,
    run_counter: Arc<AtomicU64>,
    event_counter: Arc<AtomicU64>,
}

impl AgentRuntime {
    pub fn builder() -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::default()
    }

    pub async fn run(&self, input: impl Into<AgentInput>) -> Result<RunReport> {
        self.run_with_options(
            Some(input.into()),
            AgentState::default(),
            None,
            CancellationToken::new(),
            None,
        )
        .await
    }

    pub async fn run_with_events<F, Fut>(
        &self,
        input: impl Into<AgentInput>,
        sink: F,
    ) -> Result<RunReport>
    where
        F: Fn(AgentEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.run_with_event_sink(
            input.into(),
            Arc::new(move |event| Box::pin(sink(event))),
            CancellationToken::new(),
        )
        .await
    }

    pub async fn run_with_event_sink(
        &self,
        input: AgentInput,
        sink: AgentEventSink,
        cancellation: CancellationToken,
    ) -> Result<RunReport> {
        self.run_with_options(
            Some(input),
            AgentState::default(),
            Some(sink),
            cancellation,
            None,
        )
        .await
    }

    pub async fn run_from_state(
        &self,
        input: impl Into<AgentInput>,
        initial_state: AgentState,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
    ) -> Result<RunReport> {
        self.run_with_options(Some(input.into()), initial_state, sink, cancellation, None)
            .await
    }

    pub async fn continue_from_state(
        &self,
        initial_state: AgentState,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
    ) -> Result<RunReport> {
        self.run_with_options(None, initial_state, sink, cancellation, None)
            .await
    }

    pub async fn run_from_state_with_queues(
        &self,
        input: impl Into<AgentInput>,
        initial_state: AgentState,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
        queues: Arc<dyn RuntimeQueues>,
    ) -> Result<RunReport> {
        self.run_with_options(
            Some(input.into()),
            initial_state,
            sink,
            cancellation,
            Some(queues),
        )
        .await
    }

    pub async fn continue_from_state_with_queues(
        &self,
        initial_state: AgentState,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
        queues: Arc<dyn RuntimeQueues>,
    ) -> Result<RunReport> {
        self.run_with_options(None, initial_state, sink, cancellation, Some(queues))
            .await
    }

    async fn run_with_options(
        &self,
        input: Option<AgentInput>,
        initial_state: AgentState,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
        queues: Option<Arc<dyn RuntimeQueues>>,
    ) -> Result<RunReport> {
        let run_id = self.next_run_id();
        let input = input.map(|input| self.normalize_input(&run_id, input));
        let mut state = initial_state;

        self.record_event(
            &mut state,
            &run_id,
            None,
            None,
            AgentEventKind::RunStarted,
            sink.as_ref(),
        )
        .await?;

        let result = self
            .run_turns(
                &run_id,
                input,
                &mut state,
                sink.as_ref(),
                cancellation,
                queues,
            )
            .await;
        match result {
            Ok(()) => {
                self.record_event(
                    &mut state,
                    &run_id,
                    None,
                    None,
                    AgentEventKind::RunCompleted,
                    sink.as_ref(),
                )
                .await?;
            }
            Err(error) => {
                match error {
                    AgentCoreError::Aborted => {
                        self.record_event(
                            &mut state,
                            &run_id,
                            None,
                            None,
                            AgentEventKind::RunAborted,
                            sink.as_ref(),
                        )
                        .await?;
                    }
                    AgentCoreError::EventSink(message) => {
                        self.record_event(
                            &mut state,
                            &run_id,
                            None,
                            None,
                            AgentEventKind::RunFailed {
                                error: format!("event sink failed: {message}"),
                            },
                            None,
                        )
                        .await?;
                        return Err(AgentCoreError::EventSink(message));
                    }
                    error => {
                        let message = error.to_string();
                        self.record_event(
                            &mut state,
                            &run_id,
                            None,
                            None,
                            AgentEventKind::RunFailed { error: message },
                            sink.as_ref(),
                        )
                        .await?;
                        return Err(error);
                    }
                }
                return Err(AgentCoreError::Aborted);
            }
        }

        let events = self.event_store.load(&run_id).await?;
        let replayed_state = if state.messages.is_empty()
            && state.context.is_empty()
            && state.available_tools.is_empty()
        {
            reduce_events(&events)?
        } else {
            state
        };
        Ok(RunReport {
            run_id,
            events,
            state: replayed_state,
        })
    }

    pub fn max_turns(&self) -> u64 {
        self.max_turns
    }

    pub fn context_providers(&self) -> &[Arc<dyn ContextProvider>] {
        &self.context_providers
    }

    pub fn tool_specs(&self) -> Vec<Arc<dyn ToolProvider>> {
        self.tools.values().cloned().collect()
    }

    pub fn default_model_provider(&self) -> Result<Arc<dyn ModelProvider>> {
        self.model_providers
            .get(&self.default_model_provider)
            .cloned()
            .ok_or_else(|| {
                AgentCoreError::MissingModelProvider(self.default_model_provider.clone())
            })
    }

    pub fn tool(&self, name: &str) -> Result<Arc<dyn ToolProvider>> {
        self.tools
            .get(name)
            .cloned()
            .ok_or_else(|| AgentCoreError::MissingTool(name.to_string()))
    }

    pub fn tool_execution_mode(&self) -> ToolExecutionMode {
        self.tool_execution_mode
    }

    pub fn tool_hooks(&self) -> Vec<Arc<dyn ToolCallHook>> {
        self.tool_hooks.clone()
    }

    pub fn phase_hooks(&self) -> &[Arc<dyn PhaseHook>] {
        &self.phase_hooks
    }

    pub(crate) fn context_compaction(&self) -> Option<&ContextCompactionRuntime> {
        self.context_compaction.as_ref()
    }

    pub(crate) fn tool_handles(&self) -> ToolRuntimeHandles {
        ToolRuntimeHandles {
            tools: self.tools.clone(),
            hooks: self.tool_hooks.clone(),
        }
    }

    async fn run_turns(
        &self,
        run_id: &str,
        input: Option<AgentMessage>,
        state: &mut AgentState,
        sink: Option<&AgentEventSink>,
        cancellation: CancellationToken,
        queues: Option<Arc<dyn RuntimeQueues>>,
    ) -> Result<()> {
        let mut turn_id = 1;
        let mut scratch = PhaseScratch {
            input,
            ..PhaseScratch::default()
        };

        loop {
            cancellation.throw_if_cancelled()?;
            self.record_event(
                state,
                run_id,
                Some(turn_id),
                None,
                AgentEventKind::TurnStarted,
                sink,
            )
            .await?;

            for phase in &self.phases {
                let phase_id = phase.id().to_string();
                self.record_event(
                    state,
                    run_id,
                    Some(turn_id),
                    Some(&phase_id),
                    AgentEventKind::PhaseStarted {
                        phase: phase_id.clone(),
                    },
                    sink,
                )
                .await?;

                let model_stream_sink =
                    self.model_stream_sink(run_id, turn_id, &phase_id, sink.cloned());
                let context = PhaseContext {
                    runtime: self,
                    run_id,
                    turn_id,
                    state: state.clone(),
                    scratch,
                    cancellation: cancellation.clone(),
                    model_stream_sink: Some(model_stream_sink),
                };
                let output = match phase.run(context).await {
                    Ok(output) => output,
                    Err(error) => {
                        self.record_event(
                            state,
                            run_id,
                            Some(turn_id),
                            Some(&phase_id),
                            AgentEventKind::PhaseFailed {
                                phase: phase_id.clone(),
                                error: error.to_string(),
                            },
                            sink,
                        )
                        .await?;
                        return Err(error);
                    }
                };

                for event in &output.stream_events {
                    self.record_event(
                        state,
                        run_id,
                        Some(turn_id),
                        Some(&phase_id),
                        AgentEventKind::ModelStreamEvent {
                            provider: self.default_model_provider.clone(),
                            event: event.clone(),
                        },
                        sink,
                    )
                    .await?;
                }
                for tool_call in &output.resolved_tool_calls {
                    self.record_event(
                        state,
                        run_id,
                        Some(turn_id),
                        Some(&phase_id),
                        AgentEventKind::ToolCallResolved {
                            tool_call: tool_call.clone(),
                        },
                        sink,
                    )
                    .await?;
                }
                let completed_tool_outputs = if output.completed_tool_outputs.is_empty() {
                    &output.tool_outputs
                } else {
                    &output.completed_tool_outputs
                };
                for (tool_call, tool_output) in completed_tool_outputs {
                    self.record_event(
                        state,
                        run_id,
                        Some(turn_id),
                        Some(&phase_id),
                        AgentEventKind::ToolExecutionStarted {
                            tool_call_id: tool_call.id.clone(),
                            tool_name: tool_call.name.clone(),
                        },
                        sink,
                    )
                    .await?;
                    for update in &tool_output.updates {
                        self.record_event(
                            state,
                            run_id,
                            Some(turn_id),
                            Some(&phase_id),
                            AgentEventKind::ToolExecutionUpdate {
                                tool_call_id: tool_call.id.clone(),
                                update: update.clone(),
                            },
                            sink,
                        )
                        .await?;
                    }
                    self.record_event(
                        state,
                        run_id,
                        Some(turn_id),
                        Some(&phase_id),
                        AgentEventKind::ToolExecutionCompleted {
                            tool_call_id: tool_call.id.clone(),
                            output: tool_output.clone(),
                        },
                        sink,
                    )
                    .await?;
                }
                for effect in output.effects {
                    self.commit_effect(state, run_id, Some(turn_id), Some(&phase_id), effect, sink)
                        .await?;
                }

                scratch = output.scratch;
                self.record_event(
                    state,
                    run_id,
                    Some(turn_id),
                    Some(&phase_id),
                    AgentEventKind::PhaseCompleted {
                        phase: phase_id.clone(),
                    },
                    sink,
                )
                .await?;
            }

            let decision = scratch.decision.clone().unwrap_or(TurnDecision::Stop);
            self.record_event(
                state,
                run_id,
                Some(turn_id),
                None,
                AgentEventKind::TurnCompleted {
                    decision: decision.clone(),
                },
                sink,
            )
            .await?;

            if let Some(queues) = &queues {
                let steering = queues.steering_messages().await?;
                if !steering.is_empty() {
                    self.commit_queued_messages(state, run_id, turn_id, steering, sink)
                        .await?;
                    turn_id += 1;
                    scratch = PhaseScratch::default();
                    continue;
                }
            }

            if decision == TurnDecision::Stop {
                if let Some(queues) = &queues {
                    let follow_up = queues.follow_up_messages().await?;
                    if !follow_up.is_empty() {
                        self.commit_queued_messages(state, run_id, turn_id, follow_up, sink)
                            .await?;
                        turn_id += 1;
                        scratch = PhaseScratch::default();
                        continue;
                    }
                }
                break;
            }

            turn_id += 1;
            scratch = PhaseScratch::default();
        }
        Ok(())
    }

    async fn commit_queued_messages(
        &self,
        state: &mut AgentState,
        run_id: &str,
        turn_id: u64,
        messages: Vec<AgentMessage>,
        sink: Option<&AgentEventSink>,
    ) -> Result<()> {
        for message in messages {
            self.commit_effect(
                state,
                run_id,
                Some(turn_id),
                None,
                AgentEffect::AppendMessage { message },
                sink,
            )
            .await?;
        }
        Ok(())
    }

    async fn commit_effect(
        &self,
        state: &mut AgentState,
        run_id: &str,
        turn_id: Option<u64>,
        phase: Option<&str>,
        effect: AgentEffect,
        sink: Option<&AgentEventSink>,
    ) -> Result<()> {
        self.record_event(
            state,
            run_id,
            turn_id,
            phase,
            AgentEventKind::EffectProposed {
                effect: effect.clone(),
            },
            sink,
        )
        .await?;

        match validate_effect_for_state(state, &effect) {
            Ok(()) => {
                self.record_event(
                    state,
                    run_id,
                    turn_id,
                    phase,
                    AgentEventKind::EffectCommitted { effect },
                    sink,
                )
                .await
            }
            Err(error) => {
                self.record_event(
                    state,
                    run_id,
                    turn_id,
                    phase,
                    AgentEventKind::EffectRejected {
                        effect,
                        reason: error.to_string(),
                    },
                    sink,
                )
                .await?;
                Err(error)
            }
        }
    }

    async fn record_event(
        &self,
        state: &mut AgentState,
        run_id: &str,
        turn_id: Option<u64>,
        phase: Option<&str>,
        kind: AgentEventKind,
        sink: Option<&AgentEventSink>,
    ) -> Result<()> {
        let event = AgentEvent {
            sequence: self.event_counter.fetch_add(1, Ordering::SeqCst) + 1,
            run_id: run_id.to_string(),
            turn_id,
            phase: phase.map(ToOwned::to_owned),
            kind,
        };
        self.event_store.append(event.clone()).await?;
        apply_event(state, &event)?;
        if let Some(sink) = sink {
            sink(event)
                .await
                .map_err(|error| AgentCoreError::EventSink(error.to_string()))?;
        }
        Ok(())
    }

    fn model_stream_sink(
        &self,
        run_id: &str,
        turn_id: u64,
        phase: &str,
        sink: Option<AgentEventSink>,
    ) -> ModelStreamSink {
        let run_id = run_id.to_string();
        let phase = phase.to_string();
        let event_store = Arc::clone(&self.event_store);
        let event_counter = Arc::clone(&self.event_counter);
        let provider = self.default_model_provider.clone();
        Arc::new(move |model_event: ModelStreamEvent| {
            let run_id = run_id.clone();
            let phase = phase.clone();
            let event_store = Arc::clone(&event_store);
            let event_counter = Arc::clone(&event_counter);
            let provider = provider.clone();
            let sink = sink.clone();
            Box::pin(async move {
                let event = AgentEvent {
                    sequence: event_counter.fetch_add(1, Ordering::SeqCst) + 1,
                    run_id,
                    turn_id: Some(turn_id),
                    phase: Some(phase),
                    kind: AgentEventKind::ModelStreamEvent {
                        provider,
                        event: model_event,
                    },
                };
                event_store.append(event.clone()).await?;
                if let Some(sink) = sink {
                    sink(event)
                        .await
                        .map_err(|error| AgentCoreError::EventSink(error.to_string()))?;
                }
                Ok(())
            })
        })
    }

    fn next_run_id(&self) -> String {
        let id = self.run_counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("run-{id}")
    }

    fn normalize_input(&self, run_id: &str, input: AgentInput) -> AgentMessage {
        match input {
            AgentInput::Text(text) => AgentMessage::user(format!("user-{run_id}-1"), text),
            AgentInput::Message(message) => message,
        }
    }
}

pub struct AgentRuntimeBuilder {
    event_store: Arc<dyn EventStore>,
    phases: Vec<Arc<dyn PhaseNode>>,
    model_providers: BTreeMap<String, Arc<dyn ModelProvider>>,
    default_model_provider: Option<String>,
    tools: BTreeMap<String, Arc<dyn ToolProvider>>,
    tool_execution_mode: ToolExecutionMode,
    tool_hooks: Vec<Arc<dyn ToolCallHook>>,
    phase_hooks: Vec<Arc<dyn PhaseHook>>,
    context_providers: Vec<Arc<dyn ContextProvider>>,
    compaction_summarizers: BTreeMap<String, Arc<dyn CompactionSummarizer>>,
    context_compaction: Option<ContextCompactionRegistration>,
    stdio_extensions: Vec<Arc<StdioExtension>>,
    max_turns: u64,
}

impl Default for AgentRuntimeBuilder {
    fn default() -> Self {
        Self {
            event_store: Arc::new(InMemoryEventStore::new()),
            phases: default_phases(),
            model_providers: BTreeMap::new(),
            default_model_provider: None,
            tools: BTreeMap::new(),
            tool_execution_mode: ToolExecutionMode::Parallel,
            tool_hooks: Vec::new(),
            phase_hooks: Vec::new(),
            context_providers: Vec::new(),
            compaction_summarizers: BTreeMap::new(),
            context_compaction: None,
            stdio_extensions: Vec::new(),
            max_turns: 8,
        }
    }
}

impl AgentRuntimeBuilder {
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore>) -> Self {
        self.event_store = event_store;
        self
    }

    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        let id = provider.id().to_string();
        if self.default_model_provider.is_none() {
            self.default_model_provider = Some(id.clone());
        }
        self.model_providers.insert(id, provider);
        self
    }

    pub fn default_model_provider(mut self, id: impl Into<String>) -> Self {
        self.default_model_provider = Some(id.into());
        self
    }

    pub fn with_tool(mut self, tool: Arc<dyn ToolProvider>) -> Self {
        self.tools.insert(tool.spec().name.clone(), tool);
        self
    }

    pub fn with_tool_execution_mode(mut self, mode: ToolExecutionMode) -> Self {
        self.tool_execution_mode = mode;
        self
    }

    pub fn with_tool_hook(mut self, hook: Arc<dyn ToolCallHook>) -> Self {
        self.tool_hooks.push(hook);
        self
    }

    pub fn with_phase_hook(mut self, hook: Arc<dyn PhaseHook>) -> Self {
        self.phase_hooks.push(hook);
        self
    }

    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.context_providers.push(provider);
        self
    }

    pub fn with_context_compaction(
        self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
    ) -> Self {
        self.with_context_compaction_estimator(
            config,
            summarizer,
            Arc::new(HeuristicTokenEstimator),
        )
    }

    pub fn with_context_compaction_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.context_compaction = Some(ContextCompactionRegistration::Direct {
            config,
            summarizer,
            estimator,
        });
        self
    }

    pub fn with_context_compaction_summarizer_id(
        self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
    ) -> Self {
        self.with_context_compaction_summarizer_id_and_estimator(
            config,
            summarizer_id,
            Arc::new(HeuristicTokenEstimator),
        )
    }

    pub fn with_context_compaction_summarizer_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.context_compaction = Some(ContextCompactionRegistration::SummarizerId {
            config,
            summarizer_id: summarizer_id.into(),
            estimator,
        });
        self
    }

    pub fn replace_phase(mut self, phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        if let Some(existing) = self.phases.iter_mut().find(|node| node.id() == phase_id) {
            *existing = phase;
        } else {
            self.phases.push(phase);
        }
        self
    }

    pub fn insert_phase_after(mut self, after_phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        if let Some(index) = self
            .phases
            .iter()
            .position(|node| node.id() == after_phase_id)
        {
            self.phases.insert(index + 1, phase);
        } else {
            self.phases.push(phase);
        }
        self
    }

    pub fn max_turns(mut self, max_turns: u64) -> Self {
        self.max_turns = max_turns.max(1);
        self
    }

    pub async fn with_stdio_extension(mut self, config: StdioExtensionConfig) -> Result<Self> {
        let extension = Arc::new(StdioExtension::connect(config).await?);
        let capabilities = extension.capabilities().await?;
        self.validate_extension_capabilities(&capabilities)?;
        for capability in capabilities {
            match capability {
                crate::ExtensionCapability::ModelProvider { id } => {
                    let provider = Arc::new(crate::jsonrpc::StdioModelProvider::new(
                        extension.clone(),
                        id.clone(),
                    ));
                    if self.default_model_provider.is_none() {
                        self.default_model_provider = Some(id.clone());
                    }
                    self.model_providers.insert(id, provider);
                }
                crate::ExtensionCapability::Tool { spec } => {
                    self.tools.insert(
                        spec.name.clone(),
                        Arc::new(crate::jsonrpc::StdioToolProvider::new(
                            extension.clone(),
                            spec,
                        )),
                    );
                }
                crate::ExtensionCapability::ContextProvider { id } => {
                    self.context_providers.push(Arc::new(
                        crate::jsonrpc::StdioContextProvider::new(extension.clone(), id),
                    ));
                }
                crate::ExtensionCapability::PhaseNode { id } => {
                    let phase =
                        Arc::new(crate::jsonrpc::StdioPhaseNode::new(extension.clone(), id));
                    insert_before_phase(&mut self.phases, PHASE_TURN_DECISION, phase);
                }
                crate::ExtensionCapability::PhaseHook { id } => {
                    self.phase_hooks
                        .push(Arc::new(crate::jsonrpc::StdioPhaseHook::new(
                            extension.clone(),
                            id,
                        )));
                }
                crate::ExtensionCapability::CompactionSummarizer { id } => {
                    self.compaction_summarizers.insert(
                        id.clone(),
                        Arc::new(crate::jsonrpc::StdioCompactionSummarizer::new(
                            extension.clone(),
                            id,
                        )),
                    );
                }
            }
        }
        self.stdio_extensions.push(extension);
        Ok(self)
    }

    fn validate_extension_capabilities(
        &self,
        capabilities: &[crate::ExtensionCapability],
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for capability in capabilities {
            match capability {
                crate::ExtensionCapability::ModelProvider { id } => ensure_unique_capability(
                    &mut seen,
                    "model provider",
                    id,
                    self.model_providers.contains_key(id),
                )?,
                crate::ExtensionCapability::Tool { spec } => ensure_unique_capability(
                    &mut seen,
                    "tool",
                    &spec.name,
                    self.tools.contains_key(&spec.name),
                )?,
                crate::ExtensionCapability::ContextProvider { id } => ensure_unique_capability(
                    &mut seen,
                    "context provider",
                    id,
                    self.context_providers
                        .iter()
                        .any(|provider| provider.id() == id),
                )?,
                crate::ExtensionCapability::PhaseNode { id } => ensure_unique_capability(
                    &mut seen,
                    "phase",
                    id,
                    self.phases.iter().any(|phase| phase.id() == id),
                )?,
                crate::ExtensionCapability::PhaseHook { id } => ensure_unique_capability(
                    &mut seen,
                    "phase hook",
                    id,
                    self.phase_hooks
                        .iter()
                        .any(|hook| hook.id().is_some_and(|hook_id| hook_id == id.as_str())),
                )?,
                crate::ExtensionCapability::CompactionSummarizer { id } => {
                    ensure_unique_capability(
                        &mut seen,
                        "compaction summarizer",
                        id,
                        self.compaction_summarizers.contains_key(id),
                    )?
                }
            }
        }
        Ok(())
    }

    pub fn build(self) -> Result<AgentRuntime> {
        let default_model_provider = self.default_model_provider.ok_or_else(|| {
            AgentCoreError::MissingModelProvider("no default model provider registered".into())
        })?;
        if !self.model_providers.contains_key(&default_model_provider) {
            return Err(AgentCoreError::MissingModelProvider(default_model_provider));
        }
        let context_compaction =
            resolve_context_compaction(self.context_compaction, &self.compaction_summarizers)?;
        let mut phases = self.phases;
        if context_compaction.is_some() {
            ensure_context_compaction_phase(&mut phases);
        }
        Ok(AgentRuntime {
            event_store: self.event_store,
            phases,
            model_providers: self.model_providers,
            default_model_provider,
            tools: self.tools,
            tool_execution_mode: self.tool_execution_mode,
            tool_hooks: self.tool_hooks,
            phase_hooks: self.phase_hooks,
            context_providers: self.context_providers,
            context_compaction,
            _stdio_extensions: self.stdio_extensions,
            max_turns: self.max_turns,
            run_counter: Arc::new(AtomicU64::new(0)),
            event_counter: Arc::new(AtomicU64::new(0)),
        })
    }
}

fn default_phases() -> Vec<Arc<dyn PhaseNode>> {
    vec![
        Arc::new(StandardPhase::InputIngest),
        Arc::new(StandardPhase::ContextPrepare),
        Arc::new(StandardPhase::ModelRequestPrepare),
        Arc::new(StandardPhase::ModelStream),
        Arc::new(StandardPhase::AssistantCommit),
        Arc::new(StandardPhase::ToolCallResolve),
        Arc::new(StandardPhase::ToolExecute),
        Arc::new(StandardPhase::TurnDecision),
    ]
}

fn ensure_context_compaction_phase(phases: &mut Vec<Arc<dyn PhaseNode>>) {
    if phases.iter().any(|node| node.id() == PHASE_CONTEXT_COMPACT) {
        return;
    }
    insert_before_phase(
        phases,
        PHASE_MODEL_REQUEST_PREPARE,
        Arc::new(StandardPhase::ContextCompact),
    );
}

fn insert_before_phase(
    phases: &mut Vec<Arc<dyn PhaseNode>>,
    before_phase_id: &str,
    phase: Arc<dyn PhaseNode>,
) {
    if let Some(index) = phases.iter().position(|node| node.id() == before_phase_id) {
        phases.insert(index, phase);
    } else {
        phases.push(phase);
    }
}

fn ensure_unique_capability<'a>(
    seen: &mut BTreeSet<(&'static str, &'a str)>,
    kind: &'static str,
    id: &'a str,
    exists: bool,
) -> Result<()> {
    if exists || !seen.insert((kind, id)) {
        return Err(duplicate_extension_capability(kind, id));
    }
    Ok(())
}

fn duplicate_extension_capability(kind: &str, id: &str) -> AgentCoreError {
    AgentCoreError::JsonRpc(format!("duplicate extension {kind}: {id}"))
}

#[allow(dead_code)]
fn _standard_phase_ids() -> [&'static str; 9] {
    [
        PHASE_INPUT_INGEST,
        PHASE_CONTEXT_PREPARE,
        PHASE_CONTEXT_COMPACT,
        PHASE_MODEL_REQUEST_PREPARE,
        PHASE_MODEL_STREAM,
        PHASE_ASSISTANT_COMMIT,
        PHASE_TOOL_CALL_RESOLVE,
        PHASE_TOOL_EXECUTE,
        PHASE_TURN_DECISION,
    ]
}

fn resolve_context_compaction(
    registration: Option<ContextCompactionRegistration>,
    summarizers: &BTreeMap<String, Arc<dyn CompactionSummarizer>>,
) -> Result<Option<ContextCompactionRuntime>> {
    let Some(registration) = registration else {
        return Ok(None);
    };
    match registration {
        ContextCompactionRegistration::Direct {
            config,
            summarizer,
            estimator,
        } => {
            config.validate()?;
            Ok(Some(ContextCompactionRuntime {
                config,
                summarizer,
                estimator,
            }))
        }
        ContextCompactionRegistration::SummarizerId {
            config,
            summarizer_id,
            estimator,
        } => {
            config.validate()?;
            let summarizer = summarizers.get(&summarizer_id).cloned().ok_or_else(|| {
                AgentCoreError::Phase(format!("compaction summarizer not found: {summarizer_id}"))
            })?;
            Ok(Some(ContextCompactionRuntime {
                config,
                summarizer,
                estimator,
            }))
        }
    }
}
