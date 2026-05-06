use crate::{
    bridge::{TelegramBridge, TelegramBridgeResult},
    i18n::TelegramUiCatalog,
    telegram_api::{
        TelegramInlineKeyboardButton, TelegramInlineKeyboardMarkup, TelegramMessageHandle,
    },
};
use noloong_agent_core::{
    ToolApprovalRequest, ToolPermissionDecision, ToolPermissionOutcome, ToolPermissionRequirement,
};
use serde_json::Value;
use std::collections::BTreeMap;

const ALLOW_PREFIX: &str = "ap:a:";
const DENY_PREFIX: &str = "ap:d:";

#[derive(Clone, Debug, Default)]
pub struct TelegramApprovalStore {
    next_id: u64,
    approvals: BTreeMap<String, TelegramApprovalTarget>,
}

impl TelegramApprovalStore {
    pub fn allocate_buttons(&mut self) -> TelegramApprovalButtons {
        self.next_id += 1;
        TelegramApprovalButtons {
            key: base36(self.next_id),
        }
    }

    pub fn insert_target(
        &mut self,
        buttons: &TelegramApprovalButtons,
        session_id: String,
        approval_id: String,
        message: TelegramMessageHandle,
    ) {
        self.approvals.insert(
            buttons.key.clone(),
            TelegramApprovalTarget {
                session_id,
                approval_id,
                message,
            },
        );
    }

    pub fn resolve(&mut self, data: &str) -> Option<TelegramApprovalSelection> {
        let (outcome, key) = callback_outcome(data)?;
        let target = self.approvals.remove(key)?;
        Some(TelegramApprovalSelection { outcome, target })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramApprovalButtons {
    key: String,
}

impl TelegramApprovalButtons {
    pub fn markup(&self, catalog: TelegramUiCatalog) -> TelegramInlineKeyboardMarkup {
        TelegramInlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                TelegramInlineKeyboardButton {
                    text: catalog.approval_allow_button().into(),
                    callback_data: format!("{ALLOW_PREFIX}{}", self.key),
                },
                TelegramInlineKeyboardButton {
                    text: catalog.approval_deny_button().into(),
                    callback_data: format!("{DENY_PREFIX}{}", self.key),
                },
            ]],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramApprovalSelection {
    pub outcome: ToolPermissionOutcome,
    pub target: TelegramApprovalTarget,
}

impl TelegramApprovalSelection {
    pub async fn apply(
        self,
        bridge: &TelegramBridge,
        user_id: u64,
        catalog: TelegramUiCatalog,
    ) -> TelegramBridgeResult<noloong_agent::interaction::InteractionSessionDescriptor> {
        bridge
            .resolve_approval(
                &self.target.session_id,
                &self.target.approval_id,
                ToolPermissionDecision {
                    outcome: self.outcome,
                    reason: Some(catalog.approval_resolution_reason().into()),
                    approver: Some(format!("telegram:{user_id}")),
                    metadata: Value::Object(Default::default()),
                },
            )
            .await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramApprovalTarget {
    pub session_id: String,
    pub approval_id: String,
    pub message: TelegramMessageHandle,
}

pub fn render_approval_request(
    approval: &ToolApprovalRequest,
    catalog: TelegramUiCatalog,
) -> String {
    let mut lines = vec![catalog.approval_request_title(&approval.tool_call.name)];
    if let Some(prompt) = &approval.request.prompt {
        lines.push(prompt.clone());
    }
    if let Some(reason) = &approval.request.reason {
        lines.push(catalog.approval_reason(reason));
    }
    let permissions = render_permissions(&approval.permissions);
    if !permissions.is_empty() {
        lines.push(catalog.approval_permissions(&permissions));
    }
    lines.join("\n")
}

fn callback_outcome(data: &str) -> Option<(ToolPermissionOutcome, &str)> {
    data.strip_prefix(ALLOW_PREFIX)
        .map(|key| (ToolPermissionOutcome::Allow, key))
        .or_else(|| {
            data.strip_prefix(DENY_PREFIX)
                .map(|key| (ToolPermissionOutcome::Deny, key))
        })
}

fn render_permissions(permissions: &[ToolPermissionRequirement]) -> String {
    permissions
        .iter()
        .map(|permission| permission.capability.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let mut chars = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        chars.push(match digit {
            0..=9 => (b'0' + digit) as char,
            _ => (b'a' + digit - 10) as char,
        });
        value /= 36;
    }
    chars.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::{TelegramApprovalStore, render_approval_request};
    use crate::i18n::TelegramUiCatalog;
    use crate::telegram_api::TelegramMessageHandle;
    use noloong_agent::Locale;
    use noloong_agent_core::{
        ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall, ToolPermissionOutcome,
        ToolPermissionRequirement,
    };
    use serde_json::{Map, Value, json};

    #[test]
    fn approval_button_allow_uses_short_callback_data() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        store.insert_target(
            &buttons,
            "session-1".into(),
            "approval-id-that-can-be-long".into(),
            TelegramMessageHandle {
                chat_id: 42,
                message_id: 9,
            },
        );
        let markup = buttons.markup(TelegramUiCatalog::new(Locale::En));
        let data = &markup.inline_keyboard[0][0].callback_data;

        assert!(data.len() <= 64);
        let selection = store.resolve(data).unwrap();
        assert_eq!(selection.outcome, ToolPermissionOutcome::Allow);
        assert_eq!(selection.target.approval_id, "approval-id-that-can-be-long");
    }

    #[test]
    fn approval_button_deny_uses_short_callback_data() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        store.insert_target(
            &buttons,
            "session-1".into(),
            "approval-1".into(),
            TelegramMessageHandle {
                chat_id: 42,
                message_id: 9,
            },
        );
        let data = &buttons
            .markup(TelegramUiCatalog::new(Locale::En))
            .inline_keyboard[0][1]
            .callback_data;

        let selection = store.resolve(data).unwrap();

        assert_eq!(selection.outcome, ToolPermissionOutcome::Deny);
    }

    #[test]
    fn approval_message_renders_tool_and_permissions() {
        let approval = ToolApprovalRequest {
            approval_id: "approval-1".into(),
            tool_call: ToolCall {
                id: "tool-1".into(),
                name: "host_exec".into(),
                arguments: json!({"cmd": "ls"}),
            },
            permissions: vec![ToolPermissionRequirement {
                capability: "host.exec".into(),
                description: None,
                metadata: Value::Object(Map::new()),
            }],
            hook_id: None,
            request: ToolApprovalRequestSpec {
                prompt: Some("Run command?".into()),
                reason: Some("User requested it".into()),
                expires_at_ms: None,
                metadata: Value::Object(Map::new()),
            },
        };

        let text = render_approval_request(&approval, TelegramUiCatalog::new(Locale::En));

        assert!(text.contains("host_exec"));
        assert!(text.contains("host.exec"));
    }

    #[test]
    fn approval_buttons_render_configured_locale() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();

        let markup = buttons.markup(TelegramUiCatalog::new(Locale::Zh));

        assert_eq!(markup.inline_keyboard[0][0].text, "允许");
        assert_eq!(markup.inline_keyboard[0][1].text, "拒绝");
    }
}
