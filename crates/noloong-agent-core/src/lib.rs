//! Event-sourced, providerless agent runtime core for Noloong.

mod agent;
mod anthropic_messages;
mod chat_completions;
mod compaction;
mod error;
mod jsonrpc;
mod phase;
mod provider_utils;
mod providers;
mod reducer;
mod responses;
mod runtime;
mod sse;
mod store;
mod tool_arguments;
mod types;

pub use agent::{Agent, AgentBuilder};
pub use anthropic_messages::{
    AnthropicAuthScheme, AnthropicMessagesProvider, AnthropicMessagesProviderConfig,
    AnthropicThinkingConfig,
};
pub use chat_completions::{
    ChatAudioFormat, ChatCompletionsProvider, ChatCompletionsProviderConfig, ChatImageDetail,
    ChatOutputAudioConfig, ChatOutputModality,
};
pub use compaction::{
    COMPACTION_METADATA_KEY, CompactionDecision, CompactionPlan, CompactionSummarizer,
    CompactionSummaryRequest, CompactionSummaryResult, ContextCompactionConfig,
    ContextCompactionMode, HeuristicTokenEstimator, ModelBackedCompactionSummarizer,
    ModelBackedCompactionSummarizerConfig, TokenEstimator, compacted_messages,
    compaction_summary_message, plan_compaction, serialize_messages_for_summary,
};
pub use error::{AgentCoreError, Result};
pub use jsonrpc::{StdioExtension, StdioExtensionConfig};
pub use phase::{
    PHASE_ASSISTANT_COMMIT, PHASE_CONTEXT_COMPACT, PHASE_CONTEXT_PREPARE, PHASE_INPUT_INGEST,
    PHASE_MODEL_REQUEST_PREPARE, PHASE_MODEL_STREAM, PHASE_TOOL_CALL_RESOLVE, PHASE_TOOL_EXECUTE,
    PHASE_TURN_DECISION, PhaseContext, PhaseNode, PhaseOutput, PhaseScratch, StandardPhase,
};
pub use providers::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, BeforeAssistantCommitHookContext, BeforeAssistantCommitHookResult,
    BeforeModelRequestHookContext, BeforeModelRequestHookResult, BoxFuture, CancellationToken,
    ContextProvider, ContextRequest, EventSinkFuture, ModelProvider, ModelRequest, ModelStreamSink,
    PhaseHook, ToolCallHook, ToolProvider, ToolRequest,
};
pub use reducer::{apply_event, reduce_events};
pub use responses::{
    ResponsesApiProvider, ResponsesApiProviderConfig, ResponsesReasoningConfig,
    ResponsesReasoningEffort, ResponsesReasoningSummary,
};
pub use runtime::{
    AgentEventSink, AgentInput, AgentRuntime, AgentRuntimeBuilder, RunReport, RuntimeQueues,
};
pub use sse::SseReconnectConfig;
pub use store::{EventStore, InMemoryEventStore};
pub use types::*;
