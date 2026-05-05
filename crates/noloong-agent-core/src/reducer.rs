use crate::{
    AgentCoreError, AgentEffect, AgentEvent, AgentEventKind, AgentState, ContextPatch,
    MessageCompaction, MessageReplacement, MessageRole, Result, RunPauseReason, RunStatus,
    compacted_messages,
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
            state.pending_tool_approvals.clear();
        }
        AgentEventKind::RunAborted => {
            state.status = RunStatus::Aborted;
            state.active_phase = None;
            state.pending_tool_approvals.clear();
        }
        AgentEventKind::RunFailed { error } => {
            state.status = RunStatus::Failed;
            state.last_error = Some(error.clone());
            state.active_phase = None;
            state.pending_tool_approvals.clear();
        }
        AgentEventKind::RunPaused { reason } => {
            state.status = RunStatus::Paused;
            match reason.as_ref() {
                RunPauseReason::ToolApproval { continuation } => {
                    state.active_phase = Some(continuation.phase.clone());
                }
            }
        }
        AgentEventKind::RunResumed { .. } => {
            state.status = RunStatus::Running;
            state.last_error = None;
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
        AgentEventKind::ToolApprovalRequested { approval } => {
            state
                .pending_tool_approvals
                .insert(approval.approval_id.clone(), approval.clone());
        }
        AgentEventKind::ToolApprovalResolved { approval_id, .. }
        | AgentEventKind::ToolApprovalExpired { approval_id, .. } => {
            state.pending_tool_approvals.remove(approval_id);
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_effect(effect: &AgentEffect) -> Result<()> {
    match effect {
        AgentEffect::AppendMessage { message } => {
            validate_message_id(&message.id)?;
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
        AgentEffect::CompactMessages { compaction } => validate_message_compaction(compaction)?,
        AgentEffect::ReplaceMessages { replacement } => {
            validate_message_replacement(replacement)?;
        }
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

pub fn validate_effect_for_state(state: &AgentState, effect: &AgentEffect) -> Result<()> {
    validate_effect(effect)?;
    match effect {
        AgentEffect::CompactMessages { compaction } => {
            validate_message_compaction_for_state(state, compaction)?;
        }
        AgentEffect::ReplaceMessages { replacement } => {
            validate_message_replacement_for_state(state, replacement)?;
        }
        _ => {}
    }
    Ok(())
}

fn apply_effect(state: &mut AgentState, effect: &AgentEffect) -> Result<()> {
    validate_effect_for_state(state, effect)?;
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
        AgentEffect::CompactMessages { compaction } => {
            let retained_ids = compaction
                .retained_message_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let retained_messages = state
                .messages
                .iter()
                .filter(|message| retained_ids.contains(message.id.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            state.messages =
                compacted_messages(compaction.summary_message.clone(), &retained_messages);
        }
        AgentEffect::ReplaceMessages { replacement } => {
            state.messages = replacement.replacement_messages.clone();
        }
    }
    Ok(())
}

fn validate_message_id(id: &str) -> Result<()> {
    if id.trim().is_empty() {
        return Err(AgentCoreError::InvalidEffect(
            "message id must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_message_compaction(compaction: &MessageCompaction) -> Result<()> {
    validate_message_id(&compaction.summary_message.id)?;
    if !matches!(compaction.summary_message.role, MessageRole::System) {
        return Err(AgentCoreError::InvalidEffect(
            "compaction summary message must use system role".into(),
        ));
    }
    let retained_ids = unique_message_ids(&compaction.retained_message_ids, "retained")?;
    let dropped_ids = unique_message_ids(&compaction.dropped_message_ids, "dropped")?;
    if retained_ids.intersection(&dropped_ids).next().is_some() {
        return Err(AgentCoreError::InvalidEffect(
            "compaction retained and dropped message ids must not overlap".into(),
        ));
    }
    if retained_ids.contains(compaction.summary_message.id.as_str())
        || dropped_ids.contains(compaction.summary_message.id.as_str())
    {
        return Err(AgentCoreError::InvalidEffect(
            "compaction summary message id must be new".into(),
        ));
    }
    Ok(())
}

fn validate_message_compaction_for_state(
    state: &AgentState,
    compaction: &MessageCompaction,
) -> Result<()> {
    let existing_ids = state
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<BTreeSet<_>>();
    let retained_ids = compaction
        .retained_message_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let dropped_ids = compaction
        .dropped_message_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    for id in retained_ids.union(&dropped_ids) {
        if !existing_ids.contains(id) {
            return Err(AgentCoreError::InvalidEffect(format!(
                "compaction references unknown message id: {id}"
            )));
        }
    }
    if existing_ids.contains(compaction.summary_message.id.as_str()) {
        return Err(AgentCoreError::InvalidEffect(
            "compaction summary message id must not already exist".into(),
        ));
    }
    let covered_ids = retained_ids
        .union(&dropped_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    if covered_ids != existing_ids {
        return Err(AgentCoreError::InvalidEffect(
            "compaction retained and dropped message ids must cover current messages".into(),
        ));
    }
    Ok(())
}

fn validate_message_replacement(replacement: &MessageReplacement) -> Result<()> {
    if replacement.replacement_messages.is_empty() {
        return Err(AgentCoreError::InvalidEffect(
            "replacement messages must not be empty".into(),
        ));
    }
    let mut replacement_ids = BTreeSet::new();
    for message in &replacement.replacement_messages {
        validate_message_id(&message.id)?;
        if !replacement_ids.insert(message.id.as_str()) {
            return Err(AgentCoreError::InvalidEffect(format!(
                "duplicate replacement message id: {}",
                message.id
            )));
        }
    }
    unique_message_ids(&replacement.replaced_message_ids, "replaced")?;
    Ok(())
}

fn validate_message_replacement_for_state(
    state: &AgentState,
    replacement: &MessageReplacement,
) -> Result<()> {
    let existing_ids = state
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<BTreeSet<_>>();
    let replaced_ids = replacement
        .replaced_message_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if replaced_ids != existing_ids {
        return Err(AgentCoreError::InvalidEffect(
            "replacement message ids must cover current messages".into(),
        ));
    }
    Ok(())
}

fn unique_message_ids<'a>(ids: &'a [String], label: &str) -> Result<BTreeSet<&'a str>> {
    let mut unique = BTreeSet::new();
    for id in ids {
        validate_message_id(id)?;
        if !unique.insert(id.as_str()) {
            return Err(AgentCoreError::InvalidEffect(format!(
                "duplicate {label} compaction message id: {id}"
            )));
        }
    }
    Ok(unique)
}
