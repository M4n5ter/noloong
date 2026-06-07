use super::{DisplayEvent, InteractionUxCapabilities, protocol::InteractionDisplayNotification};
use crate::text;
use noloong_agent_core::{
    AgentEvent, AgentEventKind, AgentMessage, ContentBlock, ModelStreamEvent, RunPauseReason,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

pub(super) struct DisplayProjector {
    session_id: String,
    subscription_id: String,
    ux: InteractionUxCapabilities,
    thought_started_at: BTreeMap<String, Instant>,
}

impl DisplayProjector {
    pub(super) fn new(
        session_id: String,
        subscription_id: String,
        ux: InteractionUxCapabilities,
    ) -> Self {
        Self {
            session_id,
            subscription_id,
            ux,
            thought_started_at: BTreeMap::new(),
        }
    }

    pub(super) fn handle(&mut self, event: AgentEvent) -> Vec<InteractionDisplayNotification> {
        self.project(event)
            .into_iter()
            .map(|event| InteractionDisplayNotification {
                session_id: self.session_id.clone(),
                subscription_id: self.subscription_id.clone(),
                event,
            })
            .collect()
    }

    fn project(&mut self, event: AgentEvent) -> Vec<DisplayEvent> {
        match event.kind {
            AgentEventKind::RunStarted => vec![DisplayEvent::RunStarted {
                run_id: event.run_id,
            }],
            AgentEventKind::RunCompleted => {
                let mut events = self.complete_thought(&event.run_id);
                events.push(DisplayEvent::RunCompleted {
                    run_id: event.run_id,
                });
                events
            }
            AgentEventKind::RunAborted => {
                let mut events = self.complete_thought(&event.run_id);
                events.push(DisplayEvent::RunAborted {
                    run_id: event.run_id,
                });
                events
            }
            AgentEventKind::RunFailed { error } => {
                let mut events = self.complete_thought(&event.run_id);
                events.push(DisplayEvent::RunFailed {
                    run_id: event.run_id,
                    error,
                });
                events
            }
            AgentEventKind::RunPaused { reason } => vec![DisplayEvent::RunPaused {
                run_id: event.run_id,
                reason: display_pause_reason(&reason),
            }],
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::ThinkingDelta { delta },
                ..
            } => {
                let thought_id = display_thought_id(&event.run_id);
                let mut events = self.ensure_thought_started(&event.run_id, &thought_id);
                if let Some(text) = delta.text_delta.filter(|text| !text.is_empty()) {
                    events.push(DisplayEvent::ThoughtDelta {
                        run_id: event.run_id,
                        thought_id,
                        kind: delta.kind.as_str().into(),
                        text: truncate_text_for_ux(&text, &self.ux).0,
                    });
                }
                events
            }
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::TextDelta { text },
                ..
            } => {
                if self.ux.stream_text {
                    vec![DisplayEvent::AssistantMessageDelta {
                        run_id: event.run_id.clone(),
                        display_message_id: display_message_id(&event.run_id),
                        text: truncate_text_for_ux(&text, &self.ux).0,
                    }]
                } else {
                    Vec::new()
                }
            }
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::Finished { .. },
                ..
            } => self.complete_thought(&event.run_id),
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::AppendMessage { message },
            } if matches!(message.role, noloong_agent_core::MessageRole::Assistant) => {
                let (message, truncated) = truncate_message_for_ux(message, &self.ux);
                vec![DisplayEvent::AssistantMessageFinal {
                    run_id: event.run_id.clone(),
                    display_message_id: display_message_id(&event.run_id),
                    message,
                    truncated,
                }]
            }
            AgentEventKind::ToolExecutionStarted {
                tool_call_id,
                tool_name,
            } => vec![DisplayEvent::ToolStarted {
                tool_call_id,
                tool_name,
            }],
            AgentEventKind::ToolExecutionUpdate {
                tool_call_id,
                update,
            } => vec![DisplayEvent::ToolUpdated {
                tool_call_id,
                update,
            }],
            AgentEventKind::ToolExecutionCompleted {
                tool_call_id,
                output,
            } => vec![DisplayEvent::ToolCompleted {
                tool_call_id,
                output,
            }],
            AgentEventKind::ToolApprovalRequested { approval } => {
                vec![DisplayEvent::ApprovalRequested { approval }]
            }
            AgentEventKind::ToolApprovalResolved {
                approval_id,
                decision,
            } => vec![DisplayEvent::ApprovalResolved {
                approval_id,
                decision,
            }],
            AgentEventKind::ToolApprovalExpired {
                approval_id,
                decision,
            } => vec![DisplayEvent::ApprovalExpired {
                approval_id,
                decision,
            }],
            _ => Vec::new(),
        }
    }

    fn ensure_thought_started(&mut self, run_id: &str, thought_id: &str) -> Vec<DisplayEvent> {
        if self.thought_started_at.contains_key(thought_id) {
            return Vec::new();
        }
        self.thought_started_at
            .insert(thought_id.to_string(), Instant::now());
        vec![DisplayEvent::ThoughtStarted {
            run_id: run_id.into(),
            thought_id: thought_id.into(),
        }]
    }

    fn complete_thought(&mut self, run_id: &str) -> Vec<DisplayEvent> {
        let thought_id = display_thought_id(run_id);
        let Some(started_at) = self.thought_started_at.remove(&thought_id) else {
            return Vec::new();
        };
        vec![DisplayEvent::ThoughtCompleted {
            run_id: run_id.into(),
            thought_id,
            elapsed_ms: started_at.elapsed().as_millis() as u64,
        }]
    }
}

fn display_pause_reason(reason: &RunPauseReason) -> Value {
    match reason {
        RunPauseReason::ToolApproval { .. } => json!({"type": "tool_approval"}),
    }
}

fn truncate_message_for_ux(
    mut message: AgentMessage,
    ux: &InteractionUxCapabilities,
) -> (AgentMessage, bool) {
    let Some(max_bytes) = ux.max_message_bytes else {
        return (message, false);
    };
    let mut remaining = max_bytes;
    let mut truncated = false;
    for block in &mut message.content {
        if let ContentBlock::Text { text } = block {
            let (next, block_truncated) = truncate_text_edges(text, remaining);
            *text = next;
            truncated |= block_truncated;
            remaining = remaining.saturating_sub(text.len());
        }
    }
    (message, truncated)
}

fn truncate_text_for_ux(text: &str, ux: &InteractionUxCapabilities) -> (String, bool) {
    ux.max_message_bytes
        .map(|max_bytes| truncate_text_edges(text, max_bytes))
        .unwrap_or_else(|| (text.into(), false))
}

fn truncate_text_edges(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.into(), false);
    }
    const MARKER: &str = "\n...[truncated]...\n";
    if max_bytes <= MARKER.len() + 2 {
        return (text::prefix_to_bytes(text, max_bytes), true);
    }
    let content_bytes = max_bytes - MARKER.len();
    let head_bytes = content_bytes / 2;
    let tail_bytes = content_bytes - head_bytes;
    (
        format!(
            "{}{}{}",
            text::prefix_to_bytes(text, head_bytes),
            MARKER,
            text::suffix_to_bytes(text, tail_bytes)
        ),
        true,
    )
}

fn display_message_id(run_id: &str) -> String {
    format!("{run_id}:assistant")
}

fn display_thought_id(run_id: &str) -> String {
    format!("{run_id}:thought")
}

#[cfg(test)]
mod tests {
    use super::{DisplayEvent, DisplayProjector};
    use crate::interaction::InteractionUxCapabilities;
    use noloong_agent_core::{AgentEvent, AgentEventKind, ModelStreamEvent, ThinkingDelta};

    #[test]
    fn aborted_run_completes_active_thought_then_emits_run_aborted() {
        let mut projector = DisplayProjector::new(
            "session-1".into(),
            "subscription-1".into(),
            InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: None,
                raw_events: false,
            },
        );

        let thinking = projector.project(event(
            "run-1",
            AgentEventKind::ModelStreamEvent {
                provider: "test".into(),
                event: ModelStreamEvent::ThinkingDelta {
                    delta: ThinkingDelta::from_summary("summary"),
                },
            },
        ));
        assert!(matches!(
            thinking.as_slice(),
            [
                DisplayEvent::ThoughtStarted { .. },
                DisplayEvent::ThoughtDelta { .. }
            ]
        ));

        let aborted = projector.project(event("run-1", AgentEventKind::RunAborted));

        assert!(matches!(
            aborted.as_slice(),
            [
                DisplayEvent::ThoughtCompleted { .. },
                DisplayEvent::RunAborted { .. }
            ]
        ));
        assert!(
            aborted
                .iter()
                .all(|event| !matches!(event, DisplayEvent::RunFailed { .. }))
        );
    }

    fn event(run_id: &str, kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            sequence: 1,
            run_id: run_id.into(),
            turn_id: None,
            phase: None,
            kind,
        }
    }
}
