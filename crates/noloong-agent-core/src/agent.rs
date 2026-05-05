use crate::{
    AgentCoreError, AgentEvent, AgentEventSink, AgentInput, AgentMessage, AgentRuntime,
    AgentRuntimeBuilder, AgentState, CancellationToken, CompactionSummarizer,
    ContextCompactionConfig, ContextCompactor, ContextProvider, EventSinkFuture, ModelProvider,
    PhaseHook, QueueMode, QueuedAgentMessage, Result, RunReport, RuntimeQueues,
    StdioExtensionConfig, TokenEstimator, ToolApprovalResolution, ToolCallHook, ToolExecutionMode,
    ToolProvider, apply_event,
};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Mutex, Notify};

type AgentListener = Arc<dyn Fn(AgentEvent) -> EventSinkFuture + Send + Sync>;

#[derive(Clone)]
pub struct Agent {
    inner: Arc<AgentInner>,
}

struct AgentInner {
    runtime: Arc<AgentRuntime>,
    state: Arc<Mutex<AgentState>>,
    listeners: Arc<StdMutex<BTreeMap<u64, AgentListener>>>,
    listener_counter: AtomicU64,
    active_run: Mutex<Option<CancellationToken>>,
    steering_queue: StdMutex<PendingMessageQueue>,
    follow_up_queue: StdMutex<PendingMessageQueue>,
    idle: Notify,
}

struct PendingMessageQueue {
    mode: QueueMode,
    messages: VecDeque<QueuedAgentMessage>,
}

impl PendingMessageQueue {
    fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            messages: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, message: QueuedAgentMessage) {
        self.messages.push_back(message);
    }

    fn prepend(&mut self, messages: Vec<QueuedAgentMessage>) {
        for message in messages.into_iter().rev() {
            self.messages.push_front(message);
        }
    }

    fn clear(&mut self) {
        self.messages.clear();
    }

    fn queued_messages(&self) -> Vec<QueuedAgentMessage> {
        self.messages.iter().cloned().collect()
    }

    fn edit<F>(&mut self, edit: F)
    where
        F: FnOnce(&mut Vec<QueuedAgentMessage>),
    {
        let mut messages = self.messages.drain(..).collect::<Vec<_>>();
        edit(&mut messages);
        self.messages = messages.into();
    }

    fn drain(&mut self) -> Vec<QueuedAgentMessage> {
        match self.mode {
            QueueMode::All => self.messages.drain(..).collect(),
            QueueMode::OneAtATime => self.messages.pop_front().into_iter().collect(),
        }
    }
}

impl RuntimeQueues for AgentInner {
    fn steering_messages<'a>(&'a self) -> crate::BoxFuture<'a, Vec<QueuedAgentMessage>> {
        Box::pin(async move {
            Ok(self
                .steering_queue
                .lock()
                .expect("agent steering queue lock poisoned")
                .drain())
        })
    }

    fn follow_up_messages<'a>(&'a self) -> crate::BoxFuture<'a, Vec<QueuedAgentMessage>> {
        Box::pin(async move {
            Ok(self
                .follow_up_queue
                .lock()
                .expect("agent follow-up queue lock poisoned")
                .drain())
        })
    }

    fn prepend_follow_up_messages<'a>(
        &'a self,
        messages: Vec<QueuedAgentMessage>,
    ) -> crate::BoxFuture<'a, ()> {
        Box::pin(async move {
            self.follow_up_queue
                .lock()
                .expect("agent follow-up queue lock poisoned")
                .prepend(messages);
            Ok(())
        })
    }
}

impl Agent {
    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    pub async fn prompt(&self, input: impl Into<AgentInput>) -> Result<()> {
        self.start_run(Some(input.into())).await
    }

    pub async fn continue_run(&self) -> Result<()> {
        {
            let state = self.inner.state.lock().await;
            let Some(last_message) = state.messages.last() else {
                return Err(AgentCoreError::Phase("no messages to continue from".into()));
            };
            if matches!(last_message.role, crate::MessageRole::Assistant) {
                return Err(AgentCoreError::Phase(
                    "cannot continue from assistant message".into(),
                ));
            }
        }
        self.start_run(None).await
    }

    pub async fn resume_tool_approval(&self, resolution: ToolApprovalResolution) -> Result<()> {
        self.resume_tool_approvals(vec![resolution]).await
    }

    pub async fn resume_tool_approvals(
        &self,
        resolutions: Vec<ToolApprovalResolution>,
    ) -> Result<()> {
        let run_id = {
            let state = self.inner.state.lock().await;
            if !matches!(state.status, crate::RunStatus::Paused) {
                return Err(AgentCoreError::Phase("agent is not paused".into()));
            }
            state
                .run_id
                .clone()
                .ok_or_else(|| AgentCoreError::Phase("paused agent has no run id".into()))?
        };
        self.resume_tool_approvals_for_run(run_id, resolutions)
            .await
    }

    pub async fn resume_due_tool_approval_timeouts(&self) -> Result<()> {
        self.resume_tool_approvals(Vec::new()).await
    }

    pub async fn reset(&self) {
        let mut state = self.inner.state.lock().await;
        *state = AgentState::default();
        self.clear_all_queues();
    }

    pub async fn state(&self) -> AgentState {
        self.inner.state.lock().await.clone()
    }

    pub async fn pending_tool_approvals(
        &self,
    ) -> BTreeMap<crate::ToolApprovalId, crate::ToolApprovalRequest> {
        self.inner.state.lock().await.pending_tool_approvals.clone()
    }

    pub fn subscribe<F, Fut>(&self, listener: F) -> u64
    where
        F: Fn(AgentEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let id = self.inner.listener_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner
            .listeners
            .lock()
            .expect("agent listeners lock poisoned")
            .insert(id, Arc::new(move |event| Box::pin(listener(event))));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        self.inner
            .listeners
            .lock()
            .expect("agent listeners lock poisoned")
            .remove(&id);
    }

    pub async fn abort(&self) {
        if let Some(cancellation) = self.inner.active_run.lock().await.as_ref() {
            cancellation.cancel();
            return;
        }
        let paused_run_id = {
            let state = self.inner.state.lock().await;
            matches!(state.status, crate::RunStatus::Paused)
                .then(|| state.run_id.clone())
                .flatten()
        };
        if let Some(run_id) = paused_run_id {
            let _ = self
                .inner
                .runtime
                .abort_paused_run(run_id, Some(self.event_sink()))
                .await;
            self.inner.idle.notify_waiters();
        }
    }

    pub fn steer(&self, message: AgentMessage) {
        self.steer_queued(QueuedAgentMessage::observation(message));
    }

    pub fn steer_user_input(&self, message: AgentMessage) {
        self.steer_queued(QueuedAgentMessage::user_input(message));
    }

    pub fn steer_queued(&self, message: QueuedAgentMessage) {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .enqueue(message);
    }

    pub fn follow_up(&self, message: AgentMessage) {
        self.follow_up_queued(QueuedAgentMessage::user_input(message));
    }

    pub fn follow_up_queued(&self, message: QueuedAgentMessage) {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .enqueue(message);
    }

    pub fn queued_steering_messages(&self) -> Vec<QueuedAgentMessage> {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .queued_messages()
    }

    pub fn queued_follow_up_messages(&self) -> Vec<QueuedAgentMessage> {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .queued_messages()
    }

    pub fn edit_steering_queue<F>(&self, edit: F)
    where
        F: FnOnce(&mut Vec<QueuedAgentMessage>),
    {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .edit(edit);
    }

    pub fn edit_follow_up_queue<F>(&self, edit: F)
    where
        F: FnOnce(&mut Vec<QueuedAgentMessage>),
    {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .edit(edit);
    }

    pub fn set_steering_mode(&self, mode: QueueMode) {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .mode = mode;
    }

    pub fn set_follow_up_mode(&self, mode: QueueMode) {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .mode = mode;
    }

    pub fn clear_steering_queue(&self) {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .clear();
    }

    pub fn clear_follow_up_queue(&self) {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .clear();
    }

    pub fn clear_all_queues(&self) {
        self.clear_steering_queue();
        self.clear_follow_up_queue();
    }

    pub async fn wait_for_idle(&self) {
        loop {
            if self.inner.active_run.lock().await.is_none() {
                return;
            }
            self.inner.idle.notified().await;
        }
    }

    async fn start_run(&self, input: Option<AgentInput>) -> Result<()> {
        self.run_exclusive(
            |runtime, initial_state, sink, cancellation, queues| async move {
                match input {
                    Some(input) => {
                        runtime
                            .run_from_state_with_queues(
                                input,
                                initial_state,
                                Some(sink),
                                cancellation,
                                queues,
                            )
                            .await
                    }
                    None => {
                        runtime
                            .continue_from_state_with_queues(
                                initial_state,
                                Some(sink),
                                cancellation,
                                queues,
                            )
                            .await
                    }
                }
            },
        )
        .await
    }

    async fn resume_tool_approvals_for_run(
        &self,
        run_id: String,
        resolutions: Vec<ToolApprovalResolution>,
    ) -> Result<()> {
        self.run_exclusive(
            |runtime, _initial_state, sink, cancellation, queues| async move {
                runtime
                    .resume_tool_approvals_with_queues(
                        run_id,
                        resolutions,
                        Some(sink),
                        cancellation,
                        queues,
                    )
                    .await
            },
        )
        .await
    }

    async fn run_exclusive<F, Fut>(&self, operation: F) -> Result<()>
    where
        F: FnOnce(
            Arc<AgentRuntime>,
            AgentState,
            AgentEventSink,
            CancellationToken,
            Arc<AgentInner>,
        ) -> Fut,
        Fut: Future<Output = Result<RunReport>>,
    {
        let cancellation = CancellationToken::new();
        {
            let mut active_run = self.inner.active_run.lock().await;
            if active_run.is_some() {
                return Err(AgentCoreError::Phase("agent is already running".into()));
            }
            *active_run = Some(cancellation.clone());
        }

        let initial_state = self.inner.state.lock().await.clone();
        let sink = self.event_sink();
        let result = operation(
            Arc::clone(&self.inner.runtime),
            initial_state,
            sink,
            cancellation,
            Arc::clone(&self.inner),
        )
        .await;

        if let Ok(report) = &result {
            let mut state = self.inner.state.lock().await;
            *state = report.state.clone();
        }

        let mut active_run = self.inner.active_run.lock().await;
        *active_run = None;
        drop(active_run);
        self.inner.idle.notify_waiters();
        result.map(|_| ())
    }

    fn event_sink(&self) -> AgentEventSink {
        let state = Arc::clone(&self.inner.state);
        let listeners = Arc::clone(&self.inner.listeners);
        Arc::new(move |event| {
            let state = Arc::clone(&state);
            let listeners = Arc::clone(&listeners);
            Box::pin(async move {
                {
                    let mut state = state.lock().await;
                    apply_event(&mut state, &event)?;
                }
                let listeners = listeners
                    .lock()
                    .expect("agent listeners lock poisoned")
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                for listener in listeners {
                    listener(event.clone()).await?;
                }
                Ok(())
            })
        })
    }
}

pub struct AgentBuilder {
    runtime: Option<Arc<AgentRuntime>>,
    runtime_builder: AgentRuntimeBuilder,
    initial_state: AgentState,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self {
            runtime: None,
            runtime_builder: AgentRuntime::builder(),
            initial_state: AgentState::default(),
        }
    }
}

impl AgentBuilder {
    pub fn with_runtime(mut self, runtime: Arc<AgentRuntime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    pub fn with_initial_messages(mut self, messages: Vec<AgentMessage>) -> Self {
        self.initial_state.messages = messages;
        self
    }

    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.runtime_builder = self.runtime_builder.with_model_provider(provider);
        self
    }

    pub fn with_tool(mut self, tool: Arc<dyn ToolProvider>) -> Self {
        self.runtime_builder = self.runtime_builder.with_tool(tool);
        self
    }

    pub fn with_tool_execution_mode(mut self, mode: ToolExecutionMode) -> Self {
        self.runtime_builder = self.runtime_builder.with_tool_execution_mode(mode);
        self
    }

    pub fn with_tool_hook(mut self, hook: Arc<dyn ToolCallHook>) -> Self {
        self.runtime_builder = self.runtime_builder.with_tool_hook(hook);
        self
    }

    pub fn with_phase_hook(mut self, hook: Arc<dyn PhaseHook>) -> Self {
        self.runtime_builder = self.runtime_builder.with_phase_hook(hook);
        self
    }

    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.runtime_builder = self.runtime_builder.with_context_provider(provider);
        self
    }

    pub fn with_context_compaction(
        mut self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compaction(config, summarizer);
        self
    }

    pub fn with_context_compaction_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compaction_estimator(config, summarizer, estimator);
        self
    }

    pub fn with_context_compactor(
        mut self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compactor(config, compactor);
        self
    }

    pub fn with_context_compactor_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compactor_estimator(config, compactor, estimator);
        self
    }

    pub fn with_context_compactor_id(
        mut self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compactor_id(config, compactor_id);
        self
    }

    pub fn with_context_compactor_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compactor_id_and_estimator(config, compactor_id, estimator);
        self
    }

    pub fn with_context_compaction_summarizer_id(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compaction_summarizer_id(config, summarizer_id);
        self
    }

    pub fn with_context_compaction_summarizer_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.runtime_builder = self
            .runtime_builder
            .with_context_compaction_summarizer_id_and_estimator(config, summarizer_id, estimator);
        self
    }

    pub async fn with_stdio_extension(mut self, config: StdioExtensionConfig) -> Result<Self> {
        self.runtime_builder = self.runtime_builder.with_stdio_extension(config).await?;
        Ok(self)
    }

    pub fn build(self) -> Result<Agent> {
        let runtime = match self.runtime {
            Some(runtime) => runtime,
            None => Arc::new(self.runtime_builder.build()?),
        };
        Ok(Agent {
            inner: Arc::new(AgentInner {
                runtime,
                state: Arc::new(Mutex::new(self.initial_state)),
                listeners: Arc::new(StdMutex::new(BTreeMap::new())),
                listener_counter: AtomicU64::new(0),
                active_run: Mutex::new(None),
                steering_queue: StdMutex::new(PendingMessageQueue::new(QueueMode::OneAtATime)),
                follow_up_queue: StdMutex::new(PendingMessageQueue::new(QueueMode::OneAtATime)),
                idle: Notify::new(),
            }),
        })
    }
}
