use super::{
    AgentEventSink, AgentRuntime, PhaseRecordResult, RunFlow, RunReport, RunTurnContext,
    RunTurnCursor, RuntimeQueues,
};
use crate::phase::resume_tool_approval_continuation;
use crate::reducer::reduce_events;
use crate::{
    AgentCoreError, AgentEvent, AgentEventKind, AgentState, CancellationToken, Result,
    RunPauseReason, RunResumeReason, RunStatus, ToolApprovalContinuation,
    ToolApprovalPreflightStatus, ToolApprovalResolution, ToolPermissionDecision,
    ToolPermissionOutcome,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
struct ResolvedToolApproval {
    resolution: ToolApprovalResolution,
    expired: bool,
}

impl AgentRuntime {
    pub async fn resume_tool_approvals(
        &self,
        run_id: impl AsRef<str>,
        resolutions: Vec<ToolApprovalResolution>,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
    ) -> Result<RunReport> {
        self.resume_tool_approvals_internal(run_id.as_ref(), resolutions, sink, cancellation, None)
            .await
    }

    pub async fn resume_tool_approvals_with_queues(
        &self,
        run_id: impl AsRef<str>,
        resolutions: Vec<ToolApprovalResolution>,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
        queues: Arc<dyn RuntimeQueues>,
    ) -> Result<RunReport> {
        self.resume_tool_approvals_internal(
            run_id.as_ref(),
            resolutions,
            sink,
            cancellation,
            Some(queues),
        )
        .await
    }

    pub async fn abort_paused_run(
        &self,
        run_id: impl AsRef<str>,
        sink: Option<AgentEventSink>,
    ) -> Result<RunReport> {
        let run_id = run_id.as_ref().to_string();
        let events = self.event_store.load(&run_id).await?;
        if events.is_empty() {
            return Err(AgentCoreError::Phase(format!(
                "run {run_id} does not exist"
            )));
        }
        self.ensure_event_counter_after(&events);
        let mut state = reduce_events(&events)?;
        if !matches!(state.status, RunStatus::Paused) {
            return Err(AgentCoreError::Phase(format!("run {run_id} is not paused")));
        }
        self.record_event(
            &mut state,
            &run_id,
            None,
            None,
            AgentEventKind::RunAborted,
            sink.as_ref(),
        )
        .await?;

        let events = self.event_store.load(&run_id).await?;
        Ok(RunReport {
            run_id,
            events,
            state,
        })
    }

    async fn resume_tool_approvals_internal(
        &self,
        run_id: &str,
        resolutions: Vec<ToolApprovalResolution>,
        sink: Option<AgentEventSink>,
        cancellation: CancellationToken,
        queues: Option<Arc<dyn RuntimeQueues>>,
    ) -> Result<RunReport> {
        let run_id = run_id.to_string();
        let events = self.event_store.load(&run_id).await?;
        if events.is_empty() {
            return Err(AgentCoreError::Phase(format!(
                "run {run_id} does not exist"
            )));
        }
        self.ensure_event_counter_after(&events);
        let mut state = reduce_events(&events)?;
        if !matches!(state.status, RunStatus::Paused) {
            return Err(AgentCoreError::Phase(format!("run {run_id} is not paused")));
        }
        let continuation = latest_tool_approval_continuation(&events)?;
        let resolved_approvals =
            resolve_tool_approval_decisions(&state, &continuation, resolutions)?;
        let approval_ids = resolved_approvals
            .iter()
            .map(|approval| approval.resolution.approval_id.clone())
            .collect::<Vec<_>>();

        for approval in &resolved_approvals {
            let kind = if approval.expired {
                AgentEventKind::ToolApprovalExpired {
                    approval_id: approval.resolution.approval_id.clone(),
                    decision: approval.resolution.decision.clone(),
                }
            } else {
                AgentEventKind::ToolApprovalResolved {
                    approval_id: approval.resolution.approval_id.clone(),
                    decision: approval.resolution.decision.clone(),
                }
            };
            self.record_event(
                &mut state,
                &run_id,
                Some(continuation.turn_id),
                Some(&continuation.phase),
                kind,
                sink.as_ref(),
            )
            .await?;
        }
        self.record_event(
            &mut state,
            &run_id,
            Some(continuation.turn_id),
            Some(&continuation.phase),
            AgentEventKind::RunResumed {
                reason: RunResumeReason::ToolApproval { approval_ids },
            },
            sink.as_ref(),
        )
        .await?;

        let phase_resolutions = resolved_approvals
            .iter()
            .map(|approval| approval.resolution.clone())
            .collect::<Vec<_>>();
        let result = async {
            let phase_id = continuation.phase.clone();
            let turn_id = continuation.turn_id;
            let output = match resume_tool_approval_continuation(
                self,
                continuation.clone(),
                state.clone(),
                phase_resolutions,
                cancellation.clone(),
            )
            .await
            {
                Ok(output) => output,
                Err(error) => {
                    self.record_event(
                        &mut state,
                        &run_id,
                        Some(turn_id),
                        Some(&phase_id),
                        AgentEventKind::PhaseFailed {
                            phase: phase_id.clone(),
                            error: error.to_string(),
                        },
                        sink.as_ref(),
                    )
                    .await?;
                    return Err(error);
                }
            };

            let output = match self
                .record_phase_output(
                    &mut state,
                    &run_id,
                    turn_id,
                    &phase_id,
                    output,
                    sink.as_ref(),
                )
                .await?
            {
                PhaseRecordResult::Completed(output) => *output,
                PhaseRecordResult::Paused => return Ok(RunFlow::Paused),
            };
            self.record_event(
                &mut state,
                &run_id,
                Some(turn_id),
                Some(&phase_id),
                AgentEventKind::PhaseCompleted {
                    phase: phase_id.clone(),
                },
                sink.as_ref(),
            )
            .await?;

            let next_phase_index = self.phase_index_after(&phase_id)?;
            self.run_turns_from(
                RunTurnCursor {
                    turn_id,
                    scratch: output.scratch,
                    start_phase_index: next_phase_index,
                    record_turn_started: false,
                },
                RunTurnContext {
                    run_id: &run_id,
                    state: &mut state,
                    sink: sink.as_ref(),
                },
                cancellation,
                queues,
            )
            .await
        }
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
                let error = self
                    .record_run_error(&mut state, &run_id, sink.as_ref(), error)
                    .await?;
                return Err(error);
            }
        }

        let events = self.event_store.load(&run_id).await?;
        Ok(RunReport {
            run_id,
            events,
            state,
        })
    }
}

fn latest_tool_approval_continuation(events: &[AgentEvent]) -> Result<ToolApprovalContinuation> {
    events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            AgentEventKind::RunPaused { reason } => match reason.as_ref() {
                RunPauseReason::ToolApproval { continuation } => Some(continuation.clone()),
            },
            _ => None,
        })
        .ok_or_else(|| AgentCoreError::Phase("paused run has no tool approval continuation".into()))
}

fn resolve_tool_approval_decisions(
    state: &AgentState,
    continuation: &ToolApprovalContinuation,
    resolutions: Vec<ToolApprovalResolution>,
) -> Result<Vec<ResolvedToolApproval>> {
    let mut provided = BTreeMap::new();
    for resolution in resolutions {
        if provided
            .insert(resolution.approval_id.clone(), resolution)
            .is_some()
        {
            return Err(AgentCoreError::Phase(
                "duplicate tool approval resolution id".into(),
            ));
        }
    }

    let now_ms = current_unix_ms();
    let mut resolved = Vec::new();
    let mut missing = Vec::new();
    for approval_id in pending_tool_approval_ids(continuation) {
        if let Some(resolution) = provided.remove(approval_id) {
            resolved.push(ResolvedToolApproval {
                resolution,
                expired: false,
            });
            continue;
        }

        let Some(approval) = state.pending_tool_approvals.get(approval_id) else {
            return Err(AgentCoreError::Phase(format!(
                "pending tool approval {approval_id} is not present in state"
            )));
        };
        if approval
            .request
            .expires_at_ms
            .is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
        {
            resolved.push(ResolvedToolApproval {
                resolution: timeout_tool_approval_resolution(
                    approval_id,
                    approval.request.expires_at_ms,
                ),
                expired: true,
            });
        } else {
            missing.push(approval_id.clone());
        }
    }

    if !provided.is_empty() {
        let approval_ids = provided.keys().cloned().collect::<Vec<_>>();
        return Err(AgentCoreError::Phase(format!(
            "unknown tool approval resolution ids: {}",
            approval_ids.join(", ")
        )));
    }
    if !missing.is_empty() {
        return Err(AgentCoreError::Phase(format!(
            "missing tool approval resolutions: {}",
            missing.join(", ")
        )));
    }
    Ok(resolved)
}

fn pending_tool_approval_ids(
    continuation: &ToolApprovalContinuation,
) -> impl Iterator<Item = &String> {
    continuation
        .preflights
        .iter()
        .filter_map(|preflight| match &preflight.status {
            ToolApprovalPreflightStatus::Pending { approval_id, .. } => Some(approval_id),
            _ => None,
        })
}

fn timeout_tool_approval_resolution(
    approval_id: &str,
    expires_at_ms: Option<u64>,
) -> ToolApprovalResolution {
    ToolApprovalResolution {
        approval_id: approval_id.to_string(),
        decision: ToolPermissionDecision {
            outcome: ToolPermissionOutcome::Deny,
            reason: Some("tool approval timed out".into()),
            approver: None,
            metadata: json!({
                "timeout": true,
                "expiresAtMs": expires_at_ms,
            }),
        },
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}
