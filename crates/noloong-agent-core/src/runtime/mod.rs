mod approval;
mod builder;
mod run_loop;

use crate::{
    AgentEvent, AgentMessage, AgentState, CompactionSummarizer, ContextCompactionConfig,
    ContextProvider, EventSinkFuture, EventStore, ModelProvider, PhaseHook, PhaseNode, PhaseOutput,
    PhaseScratch, StdioExtension, TokenEstimator, ToolCallHook, ToolExecutionMode, ToolProvider,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicU64},
};

pub use builder::AgentRuntimeBuilder;

pub type AgentEventSink = Arc<dyn Fn(AgentEvent) -> EventSinkFuture + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedAgentMessage {
    pub message: AgentMessage,
    pub intent: QueuedMessageIntent,
}

impl QueuedAgentMessage {
    pub fn observation(message: AgentMessage) -> Self {
        Self {
            message,
            intent: QueuedMessageIntent::Observation,
        }
    }

    pub fn user_input(message: AgentMessage) -> Self {
        Self {
            message,
            intent: QueuedMessageIntent::UserInput,
        }
    }
}

impl From<AgentMessage> for QueuedAgentMessage {
    fn from(message: AgentMessage) -> Self {
        Self::observation(message)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuedMessageIntent {
    Observation,
    UserInput,
}

pub trait RuntimeQueues: Send + Sync {
    fn steering_messages<'a>(&'a self) -> crate::providers::BoxFuture<'a, Vec<QueuedAgentMessage>>;

    fn follow_up_messages<'a>(&'a self)
    -> crate::providers::BoxFuture<'a, Vec<QueuedAgentMessage>>;

    fn prepend_follow_up_messages<'a>(
        &'a self,
        messages: Vec<QueuedAgentMessage>,
    ) -> crate::providers::BoxFuture<'a, ()>;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunFlow {
    Completed,
    Paused,
}

enum PhaseRecordResult {
    Completed(Box<PhaseOutput>),
    Paused,
}

struct RunTurnCursor {
    turn_id: u64,
    scratch: PhaseScratch,
    start_phase_index: usize,
    record_turn_started: bool,
}

struct RunTurnContext<'a> {
    run_id: &'a str,
    state: &'a mut AgentState,
    sink: Option<&'a AgentEventSink>,
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
