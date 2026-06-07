use crate::{delivery::WeixinDelivery, text::normalize_weixin_markdown};
use noloong_agent::{Locale, interaction::DisplayEvent};
use noloong_agent_core::ToolApprovalRequest;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Debug, Default)]
pub struct WeixinDisplayState {
    approvals: BTreeMap<String, ToolApprovalRequest>,
    approval_order: Vec<String>,
}

impl WeixinDisplayState {
    pub fn remember_approval(&mut self, approval: ToolApprovalRequest) {
        if !self.approvals.contains_key(&approval.approval_id) {
            self.approval_order.push(approval.approval_id.clone());
        }
        self.approvals
            .insert(approval.approval_id.clone(), approval);
    }

    pub fn approvals(&self) -> &BTreeMap<String, ToolApprovalRequest> {
        &self.approvals
    }

    pub fn approval_index(&self, approval_id: &str) -> Option<usize> {
        self.approval_order
            .iter()
            .position(|id| id == approval_id)
            .map(|index| index + 1)
    }

    pub fn approval_id_by_index(&self, index: usize) -> Option<String> {
        index
            .checked_sub(1)
            .and_then(|index| self.approval_order.get(index))
            .cloned()
    }

    pub fn remove_approval(&mut self, approval_id: &str) {
        self.approvals.remove(approval_id);
        self.approval_order.retain(|id| id != approval_id);
    }
}

pub async fn deliver_display_event(
    state: &mut WeixinDisplayState,
    delivery: &WeixinDelivery,
    peer_id: &str,
    locale: Locale,
    event: &DisplayEvent,
) -> Result<(), WeixinDisplayError> {
    match event {
        DisplayEvent::RunStarted { .. } => {
            set_typing(delivery, peer_id, true).await;
        }
        DisplayEvent::AssistantMessageDelta { .. } => {}
        DisplayEvent::AssistantMessageFinal { message, .. } => {
            clear_typing(delivery, peer_id);
            delivery.send_agent_message(peer_id, message).await?;
        }
        DisplayEvent::ApprovalRequested { approval } => {
            state.remember_approval(approval.clone());
            let index = state.approval_index(&approval.approval_id).unwrap_or(1);
            delivery
                .send_text(peer_id, &render_approval_request(index, approval, locale))
                .await?;
        }
        DisplayEvent::ApprovalResolved { approval_id, .. }
        | DisplayEvent::ApprovalExpired { approval_id, .. } => {
            state.remove_approval(approval_id);
        }
        DisplayEvent::RunFailed { error, .. } => {
            clear_typing(delivery, peer_id);
            delivery
                .send_text(peer_id, &render_run_failed(error, locale))
                .await?;
        }
        DisplayEvent::RunAborted { .. } => {
            clear_typing(delivery, peer_id);
            delivery
                .send_text(peer_id, &render_run_aborted(locale))
                .await?;
        }
        DisplayEvent::RunPaused { reason, .. } => {
            clear_typing(delivery, peer_id);
            if state.approvals().is_empty() {
                delivery
                    .send_text(peer_id, &render_run_paused(reason, locale))
                    .await?;
            }
        }
        DisplayEvent::RunCompleted { .. } => {
            clear_typing(delivery, peer_id);
        }
        DisplayEvent::ToolStarted { .. }
        | DisplayEvent::ToolUpdated { .. }
        | DisplayEvent::ToolCompleted { .. }
        | DisplayEvent::ThoughtStarted { .. }
        | DisplayEvent::ThoughtDelta { .. }
        | DisplayEvent::ThoughtCompleted { .. }
        | DisplayEvent::RawEvent { .. } => {}
    }
    Ok(())
}

fn clear_typing(delivery: &WeixinDelivery, peer_id: &str) {
    let delivery = delivery.clone();
    let peer_id = peer_id.to_owned();
    tokio::spawn(async move {
        set_typing(&delivery, &peer_id, false).await;
    });
}

async fn set_typing(delivery: &WeixinDelivery, peer_id: &str, active: bool) {
    if let Err(error) = delivery.send_typing(peer_id, active).await {
        log::debug!("weixin typing update failed: {error}");
    }
}

pub fn render_approval_request(
    index: usize,
    approval: &ToolApprovalRequest,
    locale: Locale,
) -> String {
    match locale {
        Locale::Zh => format!(
            "需要审批 {index}\n\n工具：{}\n\n审批：{}\n\n回复“/同意 {index}”或“/拒绝 {index}”。",
            approval.tool_call.name, approval.approval_id
        ),
        _ => format!(
            "Approval required {index}\n\nTool: {}\n\nApproval: {}\n\nReply with \"/approve {index}\" or \"/deny {index}\".",
            approval.tool_call.name, approval.approval_id
        ),
    }
}

fn render_run_failed(error: &str, locale: Locale) -> String {
    match locale {
        Locale::Zh => format!("任务失败：{}", normalize_weixin_markdown(error)),
        _ => format!("Task failed: {}", normalize_weixin_markdown(error)),
    }
}

fn render_run_aborted(locale: Locale) -> String {
    match locale {
        Locale::Zh => "任务已停止。".into(),
        _ => "Task stopped.".into(),
    }
}

fn render_run_paused(reason: &serde_json::Value, locale: Locale) -> String {
    match locale {
        Locale::Zh => {
            if is_approval_pause(reason) {
                "任务已暂停，等待审批。".into()
            } else {
                "任务已暂停。".into()
            }
        }
        _ => {
            if is_approval_pause(reason) {
                "Task paused and is waiting for approval.".into()
            } else {
                "Task paused.".into()
            }
        }
    }
}

fn is_approval_pause(reason: &serde_json::Value) -> bool {
    reason
        .pointer("/continuation/preflights")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|preflights| {
            preflights.iter().any(|preflight| {
                preflight
                    .get("permissionAudit")
                    .and_then(|audit| audit.get("permissions"))
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|permissions| !permissions.is_empty())
            })
        })
}

#[derive(Debug, Error)]
pub enum WeixinDisplayError {
    #[error("{0}")]
    Delivery(#[from] crate::delivery::WeixinDeliveryError),
}

#[cfg(test)]
mod tests {
    use super::{WeixinDisplayState, render_approval_request, render_run_paused};
    use noloong_agent::Locale;
    use noloong_agent_core::{ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall};

    #[test]
    fn approval_state_removes_resolved_approval() {
        let mut state = WeixinDisplayState::default();
        state.remember_approval(approval("a1"));

        state.remove_approval("a1");

        assert!(state.approvals().is_empty());
        assert_eq!(state.approval_id_by_index(1), None);
    }

    #[test]
    fn approval_card_uses_numbered_commands() {
        let approval = approval("a1");

        let rendered = render_approval_request(2, &approval, Locale::Zh);
        assert!(rendered.contains("/同意 2"));
        assert!(rendered.contains("/拒绝 2"));
    }

    fn approval(approval_id: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            approval_id: approval_id.into(),
            tool_call: ToolCall {
                id: "t1".into(),
                name: "host.exec.start".into(),
                arguments: serde_json::json!({}),
            },
            permissions: Vec::new(),
            hook_id: None,
            request: ToolApprovalRequestSpec {
                prompt: None,
                reason: Some("test".into()),
                expires_at_ms: None,
                metadata: serde_json::Value::Object(serde_json::Map::new()),
            },
        }
    }

    #[test]
    fn run_paused_hides_internal_approval_json() {
        let reason = serde_json::json!({
            "continuation": {
                "preflights": [{
                    "permissionAudit": {
                        "permissions": [{"capability": "host.command"}]
                    }
                }]
            },
            "toolCall": {"name": "host.exec.start"}
        });

        let rendered = render_run_paused(&reason, Locale::Zh);

        assert_eq!(rendered, "任务已暂停，等待审批。");
    }
}
