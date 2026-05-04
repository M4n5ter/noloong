use crate::{
    AfterAssistantCommitHookContext, AfterAssistantCommitHookResult, AfterModelRequestHookContext,
    AfterModelRequestHookResult, AgentMessage, AgentState, BeforeAssistantCommitHookContext,
    BeforeAssistantCommitHookResult, BeforeModelRequestHookContext, BeforeModelRequestHookResult,
    ModelRequest, ModelStreamEvent, PhaseHook, Result, providers::CancellationToken,
};
use std::sync::Arc;

pub(super) struct PhaseHookRunner<'a> {
    hooks: &'a [Arc<dyn PhaseHook>],
    run_id: &'a str,
    turn_id: u64,
    state: &'a AgentState,
    cancellation: &'a CancellationToken,
}

impl<'a> PhaseHookRunner<'a> {
    pub(super) fn new(
        hooks: &'a [Arc<dyn PhaseHook>],
        run_id: &'a str,
        turn_id: u64,
        state: &'a AgentState,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            hooks,
            run_id,
            turn_id,
            state,
            cancellation,
        }
    }

    pub(super) fn has_hooks(&self) -> bool {
        !self.hooks.is_empty()
    }

    pub(super) async fn before_model_request(
        &self,
        mut request: ModelRequest,
    ) -> Result<ModelRequest> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(BeforeModelRequestHookResult { request: next }) = hook
                .before_model_request(
                    BeforeModelRequestHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        request: &request,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                request = next;
            }
        }
        Ok(request)
    }

    pub(super) async fn after_model_request(
        &self,
        request: &ModelRequest,
        mut events: Vec<ModelStreamEvent>,
    ) -> Result<Vec<ModelStreamEvent>> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(AfterModelRequestHookResult { events: next }) = hook
                .after_model_request(
                    AfterModelRequestHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        request,
                        events: &events,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                events = next;
            }
        }
        Ok(events)
    }

    pub(super) async fn before_assistant_commit(
        &self,
        mut events: Vec<ModelStreamEvent>,
    ) -> Result<Vec<ModelStreamEvent>> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(BeforeAssistantCommitHookResult { events: next }) = hook
                .before_assistant_commit(
                    BeforeAssistantCommitHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        events: &events,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                events = next;
            }
        }
        Ok(events)
    }

    pub(super) async fn after_assistant_commit(
        &self,
        mut message: AgentMessage,
    ) -> Result<AgentMessage> {
        for hook in self.hooks {
            self.cancellation.throw_if_cancelled()?;
            if let Some(AfterAssistantCommitHookResult { message: next }) = hook
                .after_assistant_commit(
                    AfterAssistantCommitHookContext {
                        run_id: self.run_id,
                        turn_id: self.turn_id,
                        state: self.state,
                        message: &message,
                    },
                    self.cancellation.clone(),
                )
                .await?
            {
                message = next;
            }
        }
        Ok(message)
    }
}
