use crate::{
    AfterToolCallContext, AfterToolCallResult, AgentEffect, AgentMessage, AgentState,
    BeforeToolCallContext, BeforeToolCallResult, ModelStreamEvent, Result, ToolOutput, ToolSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type EventSinkFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;
pub type ModelStreamSink = Arc<dyn Fn(ModelStreamEvent) -> EventSinkFuture + Send + Sync>;

pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;
    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>>;
}

pub trait ToolProvider: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput>;
}

pub trait ContextProvider: Send + Sync {
    fn id(&self) -> &str;
    fn prepare_context<'a>(
        &'a self,
        request: ContextRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<AgentEffect>>;
}

pub trait ToolCallHook: Send + Sync {
    fn id(&self) -> Option<&str> {
        None
    }

    fn before_tool_call<'a>(
        &'a self,
        _context: BeforeToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>> {
        Box::pin(async { Ok(None) })
    }

    fn after_tool_call<'a>(
        &'a self,
        _context: AfterToolCallContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterToolCallResult>> {
        Box::pin(async { Ok(None) })
    }
}

pub trait PhaseHook: Send + Sync {
    fn id(&self) -> Option<&str> {
        None
    }

    fn before_model_request<'a>(
        &'a self,
        _context: BeforeModelRequestHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async { Ok(None) })
    }

    fn after_model_request<'a>(
        &'a self,
        _context: AfterModelRequestHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterModelRequestHookResult>> {
        Box::pin(async { Ok(None) })
    }

    fn before_assistant_commit<'a>(
        &'a self,
        _context: BeforeAssistantCommitHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeAssistantCommitHookResult>> {
        Box::pin(async { Ok(None) })
    }

    fn after_assistant_commit<'a>(
        &'a self,
        _context: AfterAssistantCommitHookContext<'a>,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterAssistantCommitHookResult>> {
        Box::pin(async { Ok(None) })
    }
}

#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<CancellationInner>,
}

#[derive(Debug)]
struct CancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancellationInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::SeqCst) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    pub fn throw_if_cancelled(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(crate::AgentCoreError::Aborted)
        } else {
            Ok(())
        }
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub run_id: String,
    pub turn_id: u64,
    pub messages: Vec<AgentMessage>,
    #[serde(default)]
    pub context: Map<String, Value>,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequest {
    pub run_id: String,
    pub turn_id: u64,
    pub tool_call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
    pub state: AgentState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextRequest {
    pub run_id: String,
    pub turn_id: u64,
    pub state: AgentState,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeModelRequestHookContext<'a> {
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: &'a AgentState,
    pub request: &'a ModelRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeModelRequestHookResult {
    pub request: ModelRequest,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterModelRequestHookContext<'a> {
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: &'a AgentState,
    pub request: &'a ModelRequest,
    pub events: &'a [ModelStreamEvent],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterModelRequestHookResult {
    pub events: Vec<ModelStreamEvent>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAssistantCommitHookContext<'a> {
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: &'a AgentState,
    pub events: &'a [ModelStreamEvent],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAssistantCommitHookResult {
    pub events: Vec<ModelStreamEvent>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterAssistantCommitHookContext<'a> {
    pub run_id: &'a str,
    pub turn_id: u64,
    pub state: &'a AgentState,
    pub message: &'a AgentMessage,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AfterAssistantCommitHookResult {
    pub message: AgentMessage,
}
