//! Event-sourced, providerless agent runtime core for Noloong.

mod agent;
mod chat_completions;
mod error;
mod jsonrpc;
mod phase;
mod providers;
mod reducer;
mod runtime;
mod store;
mod types;

pub use agent::{Agent, AgentBuilder};
pub use chat_completions::{ChatCompletionsProvider, ChatCompletionsProviderConfig};
pub use error::{AgentCoreError, Result};
pub use jsonrpc::{StdioExtension, StdioExtensionConfig};
pub use phase::{
    PHASE_ASSISTANT_COMMIT, PHASE_CONTEXT_PREPARE, PHASE_INPUT_INGEST, PHASE_MODEL_REQUEST_PREPARE,
    PHASE_MODEL_STREAM, PHASE_TOOL_CALL_RESOLVE, PHASE_TOOL_EXECUTE, PHASE_TURN_DECISION,
    PhaseContext, PhaseNode, PhaseOutput, PhaseScratch, StandardPhase,
};
pub use providers::{
    BoxFuture, CancellationToken, ContextProvider, ContextRequest, EventSinkFuture, ModelProvider,
    ModelRequest, ModelStreamSink, ToolCallHook, ToolProvider, ToolRequest,
};
pub use reducer::{apply_event, reduce_events};
pub use runtime::{
    AgentEventSink, AgentInput, AgentRuntime, AgentRuntimeBuilder, RunReport, RuntimeQueues,
};
pub use store::{EventStore, InMemoryEventStore};
pub use types::*;
