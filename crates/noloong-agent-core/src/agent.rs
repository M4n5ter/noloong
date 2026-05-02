use crate::{
    AgentCoreError, AgentEvent, AgentEventSink, AgentInput, AgentMessage, AgentRuntime,
    AgentRuntimeBuilder, AgentState, CancellationToken, ContextProvider, EventSinkFuture,
    ModelProvider, QueueMode, Result, RuntimeQueues, StdioExtensionConfig, ToolCallHook,
    ToolExecutionMode, ToolProvider, apply_event,
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
    messages: VecDeque<AgentMessage>,
}

impl PendingMessageQueue {
    fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            messages: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, message: AgentMessage) {
        self.messages.push_back(message);
    }

    fn clear(&mut self) {
        self.messages.clear();
    }

    fn drain(&mut self) -> Vec<AgentMessage> {
        match self.mode {
            QueueMode::All => self.messages.drain(..).collect(),
            QueueMode::OneAtATime => self.messages.pop_front().into_iter().collect(),
        }
    }
}

impl RuntimeQueues for AgentInner {
    fn steering_messages<'a>(&'a self) -> crate::BoxFuture<'a, Vec<AgentMessage>> {
        Box::pin(async move {
            Ok(self
                .steering_queue
                .lock()
                .expect("agent steering queue lock poisoned")
                .drain())
        })
    }

    fn follow_up_messages<'a>(&'a self) -> crate::BoxFuture<'a, Vec<AgentMessage>> {
        Box::pin(async move {
            Ok(self
                .follow_up_queue
                .lock()
                .expect("agent follow-up queue lock poisoned")
                .drain())
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

    pub async fn reset(&self) {
        let mut state = self.inner.state.lock().await;
        *state = AgentState::default();
        self.clear_all_queues();
    }

    pub async fn state(&self) -> AgentState {
        self.inner.state.lock().await.clone()
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
        }
    }

    pub fn steer(&self, message: AgentMessage) {
        self.inner
            .steering_queue
            .lock()
            .expect("agent steering queue lock poisoned")
            .enqueue(message);
    }

    pub fn follow_up(&self, message: AgentMessage) {
        self.inner
            .follow_up_queue
            .lock()
            .expect("agent follow-up queue lock poisoned")
            .enqueue(message);
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
        let result = match input {
            Some(input) => {
                self.inner
                    .runtime
                    .run_from_state_with_queues(
                        input,
                        initial_state,
                        Some(sink),
                        cancellation,
                        self.inner.clone(),
                    )
                    .await
            }
            None => {
                self.inner
                    .runtime
                    .continue_from_state_with_queues(
                        initial_state,
                        Some(sink),
                        cancellation,
                        self.inner.clone(),
                    )
                    .await
            }
        };

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

    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.runtime_builder = self.runtime_builder.with_context_provider(provider);
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
