use super::{
    AgentEventSink, AgentInput, AgentRuntime, ContextCompactionRuntime, PhaseRecordResult, RunFlow,
    RunReport, RunTurnContext, RunTurnCursor, RuntimeQueues, ToolRuntimeHandles,
};
use crate::reducer::{apply_event, reduce_events, validate_effect_for_state};
use crate::{
    AgentCoreError, AgentEffect, AgentEvent, AgentEventKind, AgentMessage, AgentState,
    CancellationToken, ContextProvider, ModelProvider, ModelStreamEvent, ModelStreamSink,
    PhaseContext, PhaseHook, PhaseOutput, PhaseScratch, QueuedAgentMessage, QueuedMessageIntent,
    Result, ToolCallHook, ToolExecutionMode, ToolProvider, TurnDecision,
};
use std::{
    future::Future,
    sync::{Arc, atomic::Ordering},
};

impl AgentRuntime {
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

        if let Some(queues) = &queues {
            let steering = queues.steering_messages().await?;
            if !steering.is_empty() {
                self.commit_queued_messages(
                    &mut state,
                    &run_id,
                    None,
                    queued_messages_into_messages(steering),
                    sink.as_ref(),
                )
                .await?;
            }
        }

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
            Ok(RunFlow::Completed) => {
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
            Ok(RunFlow::Paused) => {}
            Err(error) => {
                return Err(self
                    .record_run_error(&mut state, &run_id, sink.as_ref(), error)
                    .await?);
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

    pub fn context_compaction_config(&self) -> Option<&crate::ContextCompactionConfig> {
        self.context_compaction
            .as_ref()
            .map(|compaction| &compaction.config)
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
    ) -> Result<RunFlow> {
        let turn_id = 1;
        let scratch = PhaseScratch {
            input,
            ..PhaseScratch::default()
        };
        self.run_turns_from(
            RunTurnCursor {
                turn_id,
                scratch,
                start_phase_index: 0,
                record_turn_started: true,
            },
            RunTurnContext {
                run_id,
                state,
                sink,
            },
            cancellation,
            queues,
        )
        .await
    }

    pub(super) async fn run_turns_from(
        &self,
        cursor: RunTurnCursor,
        context: RunTurnContext<'_>,
        cancellation: CancellationToken,
        queues: Option<Arc<dyn RuntimeQueues>>,
    ) -> Result<RunFlow> {
        let RunTurnContext {
            run_id,
            state,
            sink,
        } = context;
        let RunTurnCursor {
            mut turn_id,
            mut scratch,
            mut start_phase_index,
            mut record_turn_started,
        } = cursor;
        loop {
            cancellation.throw_if_cancelled()?;
            if record_turn_started {
                self.record_event(
                    state,
                    run_id,
                    Some(turn_id),
                    None,
                    AgentEventKind::TurnStarted,
                    sink,
                )
                .await?;
            }

            for phase in self.phases.iter().skip(start_phase_index) {
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

                let output = match self
                    .record_phase_output(state, run_id, turn_id, &phase_id, output, sink)
                    .await?
                {
                    PhaseRecordResult::Completed(output) => *output,
                    PhaseRecordResult::Paused => return Ok(RunFlow::Paused),
                };

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

            let mut committed_stop_steering = false;
            if let Some(queues) = &queues {
                let steering = queues.steering_messages().await?;
                if !steering.is_empty() {
                    if decision == TurnDecision::Stop {
                        let (steering, user_inputs) = split_user_input_messages(steering);
                        if !steering.is_empty() {
                            self.commit_queued_messages(
                                state,
                                run_id,
                                Some(turn_id),
                                queued_messages_into_messages(steering),
                                sink,
                            )
                            .await?;
                            committed_stop_steering = true;
                        }
                        if !user_inputs.is_empty() {
                            queues.prepend_follow_up_messages(user_inputs).await?;
                        }
                    } else {
                        self.commit_queued_messages(
                            state,
                            run_id,
                            Some(turn_id),
                            queued_messages_into_messages(steering),
                            sink,
                        )
                        .await?;
                        turn_id += 1;
                        scratch = PhaseScratch::default();
                        continue;
                    }
                }
            }

            if decision == TurnDecision::Stop {
                if let Some(queues) = &queues {
                    let follow_up = queues.follow_up_messages().await?;
                    if !follow_up.is_empty() {
                        self.commit_queued_messages(
                            state,
                            run_id,
                            Some(turn_id),
                            queued_messages_into_messages(follow_up),
                            sink,
                        )
                        .await?;
                        turn_id += 1;
                        scratch = PhaseScratch::default();
                        continue;
                    }
                }
                if committed_stop_steering {
                    turn_id += 1;
                    scratch = PhaseScratch::default();
                    continue;
                }
                break;
            }

            turn_id += 1;
            scratch = PhaseScratch::default();
            start_phase_index = 0;
            record_turn_started = true;
        }
        Ok(RunFlow::Completed)
    }

    pub(super) async fn record_phase_output(
        &self,
        state: &mut AgentState,
        run_id: &str,
        turn_id: u64,
        phase_id: &str,
        mut output: PhaseOutput,
        sink: Option<&AgentEventSink>,
    ) -> Result<PhaseRecordResult> {
        for event in &output.stream_events {
            self.record_event(
                state,
                run_id,
                Some(turn_id),
                Some(phase_id),
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
                Some(phase_id),
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
        let completed_tool_permission_audits = if output.completed_tool_permission_audits.is_empty()
        {
            &output.tool_permission_audits
        } else {
            &output.completed_tool_permission_audits
        };
        if !completed_tool_permission_audits.is_empty()
            && completed_tool_permission_audits.len() != completed_tool_outputs.len()
        {
            return Err(AgentCoreError::Phase(format!(
                "tool permission audit count {} does not match tool output count {}",
                completed_tool_permission_audits.len(),
                completed_tool_outputs.len()
            )));
        }
        for (index, (tool_call, tool_output)) in completed_tool_outputs.iter().enumerate() {
            if let Some(audit) = completed_tool_permission_audits.get(index) {
                if audit.tool_call.id != tool_call.id || audit.tool_call.name != tool_call.name {
                    return Err(AgentCoreError::Phase(format!(
                        "tool permission audit for {} does not match tool output {}",
                        audit.tool_call.id, tool_call.id
                    )));
                }
                self.record_event(
                    state,
                    run_id,
                    Some(turn_id),
                    Some(phase_id),
                    AgentEventKind::ToolPermissionRequested {
                        tool_call: audit.tool_call.clone(),
                        permissions: audit.permissions.clone(),
                    },
                    sink,
                )
                .await?;
                for record in &audit.decisions {
                    self.record_event(
                        state,
                        run_id,
                        Some(turn_id),
                        Some(phase_id),
                        AgentEventKind::ToolPermissionDecided {
                            tool_call_id: audit.tool_call.id.clone(),
                            tool_name: audit.tool_call.name.clone(),
                            hook_id: record.hook_id.clone(),
                            decision: record.decision.clone(),
                        },
                        sink,
                    )
                    .await?;
                }
            }
            self.record_event(
                state,
                run_id,
                Some(turn_id),
                Some(phase_id),
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
                    Some(phase_id),
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
                Some(phase_id),
                AgentEventKind::ToolExecutionCompleted {
                    tool_call_id: tool_call.id.clone(),
                    output: tool_output.clone(),
                },
                sink,
            )
            .await?;
        }

        let effects = std::mem::take(&mut output.effects);
        for effect in effects {
            self.commit_effect(state, run_id, Some(turn_id), Some(phase_id), effect, sink)
                .await?;
        }

        if let Some(reason) = output.pause.clone() {
            for approval in &output.tool_approval_requests {
                self.record_event(
                    state,
                    run_id,
                    Some(turn_id),
                    Some(phase_id),
                    AgentEventKind::ToolApprovalRequested {
                        approval: approval.clone(),
                    },
                    sink,
                )
                .await?;
            }
            self.record_event(
                state,
                run_id,
                Some(turn_id),
                Some(phase_id),
                AgentEventKind::RunPaused {
                    reason: Box::new(reason),
                },
                sink,
            )
            .await?;
            return Ok(PhaseRecordResult::Paused);
        }

        Ok(PhaseRecordResult::Completed(Box::new(output)))
    }

    async fn commit_queued_messages(
        &self,
        state: &mut AgentState,
        run_id: &str,
        turn_id: Option<u64>,
        messages: Vec<AgentMessage>,
        sink: Option<&AgentEventSink>,
    ) -> Result<()> {
        for message in messages {
            self.commit_effect(
                state,
                run_id,
                turn_id,
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

    pub(super) async fn record_run_error(
        &self,
        state: &mut AgentState,
        run_id: &str,
        sink: Option<&AgentEventSink>,
        error: AgentCoreError,
    ) -> Result<AgentCoreError> {
        match error {
            AgentCoreError::Aborted => {
                self.record_event(state, run_id, None, None, AgentEventKind::RunAborted, sink)
                    .await?;
                Ok(AgentCoreError::Aborted)
            }
            AgentCoreError::EventSink(message) => {
                self.record_event(
                    state,
                    run_id,
                    None,
                    None,
                    AgentEventKind::RunFailed {
                        error: format!("event sink failed: {message}"),
                    },
                    None,
                )
                .await?;
                Ok(AgentCoreError::EventSink(message))
            }
            error => {
                let message = error.to_string();
                self.record_event(
                    state,
                    run_id,
                    None,
                    None,
                    AgentEventKind::RunFailed { error: message },
                    sink,
                )
                .await?;
                Ok(error)
            }
        }
    }

    pub(super) async fn record_event(
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
        match &self.run_id_prefix {
            Some(prefix) => format!("run-{prefix}-{id}"),
            None => format!("run-{id}"),
        }
    }

    pub(super) fn ensure_event_counter_after(&self, events: &[AgentEvent]) {
        let Some(max_sequence) = events.iter().map(|event| event.sequence).max() else {
            return;
        };
        let mut current = self.event_counter.load(Ordering::SeqCst);
        while current < max_sequence {
            match self.event_counter.compare_exchange(
                current,
                max_sequence,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    pub(super) fn phase_index_after(&self, phase_id: &str) -> Result<usize> {
        let index = self
            .phases
            .iter()
            .position(|phase| phase.id() == phase_id)
            .ok_or_else(|| AgentCoreError::Phase(format!("phase {phase_id} is not registered")))?;
        Ok(index + 1)
    }

    fn normalize_input(&self, run_id: &str, input: AgentInput) -> AgentMessage {
        match input {
            AgentInput::Text(text) => AgentMessage::user(format!("user-{run_id}-1"), text),
            AgentInput::Message(message) => message,
        }
    }
}

fn queued_messages_into_messages(messages: Vec<QueuedAgentMessage>) -> Vec<AgentMessage> {
    messages.into_iter().map(|queued| queued.message).collect()
}

fn split_user_input_messages(
    messages: Vec<QueuedAgentMessage>,
) -> (Vec<QueuedAgentMessage>, Vec<QueuedAgentMessage>) {
    messages
        .into_iter()
        .partition(|message| message.intent == QueuedMessageIntent::Observation)
}
