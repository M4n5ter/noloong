use crate::{
    AgentCoreError, AgentEffect, AgentEvent, AgentEventKind, AgentState, ContextPatch, Result,
    RunStatus,
};
use std::collections::BTreeSet;

pub fn reduce_events(events: &[AgentEvent]) -> Result<AgentState> {
    let mut state = AgentState::default();
    for event in events {
        apply_event(&mut state, event)?;
    }
    Ok(state)
}

pub fn apply_event(state: &mut AgentState, event: &AgentEvent) -> Result<()> {
    match &event.kind {
        AgentEventKind::RunStarted => {
            state.run_id = Some(event.run_id.clone());
            state.status = RunStatus::Running;
            state.last_error = None;
        }
        AgentEventKind::RunCompleted => {
            state.status = RunStatus::Completed;
            state.active_phase = None;
        }
        AgentEventKind::RunAborted => {
            state.status = RunStatus::Aborted;
            state.active_phase = None;
        }
        AgentEventKind::RunFailed { error } => {
            state.status = RunStatus::Failed;
            state.last_error = Some(error.clone());
            state.active_phase = None;
        }
        AgentEventKind::TurnCompleted { .. } => {
            state.completed_turns += 1;
        }
        AgentEventKind::PhaseStarted { phase } => {
            state.active_phase = Some(phase.clone());
        }
        AgentEventKind::PhaseCompleted { phase }
            if state.active_phase.as_deref() == Some(phase.as_str()) =>
        {
            state.active_phase = None;
        }
        AgentEventKind::PhaseCompleted { .. } => {}
        AgentEventKind::PhaseFailed { error, .. } => {
            state.status = RunStatus::Failed;
            state.last_error = Some(error.clone());
            state.active_phase = None;
        }
        AgentEventKind::EffectCommitted { effect } => apply_effect(state, effect)?,
        _ => {}
    }
    Ok(())
}

pub fn validate_effect(effect: &AgentEffect) -> Result<()> {
    match effect {
        AgentEffect::AppendMessage { message } => {
            if message.id.trim().is_empty() {
                return Err(AgentCoreError::InvalidEffect(
                    "message id must not be empty".to_string(),
                ));
            }
        }
        AgentEffect::PatchContext { patch } => match patch {
            ContextPatch::Set { key, .. } | ContextPatch::Remove { key } => {
                if key.trim().is_empty() {
                    return Err(AgentCoreError::InvalidEffect(
                        "context patch key must not be empty".to_string(),
                    ));
                }
            }
        },
        AgentEffect::SetAvailableTools { tools } => {
            let mut names = BTreeSet::new();
            for tool in tools {
                if tool.name.trim().is_empty() {
                    return Err(AgentCoreError::InvalidEffect(
                        "tool name must not be empty".to_string(),
                    ));
                }
                if !names.insert(tool.name.as_str()) {
                    return Err(AgentCoreError::InvalidEffect(format!(
                        "duplicate tool name: {}",
                        tool.name
                    )));
                }
            }
        }
    }
    Ok(())
}

fn apply_effect(state: &mut AgentState, effect: &AgentEffect) -> Result<()> {
    validate_effect(effect)?;
    match effect {
        AgentEffect::AppendMessage { message } => {
            state.messages.push(message.clone());
        }
        AgentEffect::PatchContext { patch } => match patch {
            ContextPatch::Set { key, value } => {
                state.context.insert(key.clone(), value.clone());
            }
            ContextPatch::Remove { key } => {
                state.context.remove(key);
            }
        },
        AgentEffect::SetAvailableTools { tools } => {
            state.available_tools = tools
                .iter()
                .cloned()
                .map(|tool| (tool.name.clone(), tool))
                .collect();
        }
    }
    Ok(())
}
