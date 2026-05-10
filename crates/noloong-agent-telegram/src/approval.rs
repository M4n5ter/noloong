use crate::{
    bridge::{TelegramBridge, TelegramBridgeResult},
    callback::ShortCallbackStore,
    i18n::TelegramUiCatalog,
    telegram_api::{
        TelegramInlineKeyboardButton, TelegramInlineKeyboardMarkup, TelegramMessageHandle,
    },
    text::{truncate_end_chars, truncate_middle_chars},
};
use noloong_agent::{BuiltInToolName, StartCommandRequest};
use noloong_agent_core::{
    ToolApprovalRequest, ToolCall, ToolPermissionDecision, ToolPermissionOutcome,
    ToolPermissionRequirement,
};
use serde_json::Value;
use std::collections::BTreeMap;

const ALLOW_PREFIX: &str = "ap:a:";
const DENY_PREFIX: &str = "ap:d:";
const PENDING_APPROVAL_RENDER_LIMIT: usize = 20;
const APPROVAL_ARGUMENT_RENDER_LIMIT: usize = 600;
const APPROVAL_ARGUMENT_OBJECT_KEYS: usize = 8;
const APPROVAL_ARGUMENT_VALUE_LIMIT: usize = 160;

#[derive(Clone, Debug, Default)]
pub struct TelegramApprovalStore {
    approvals: ShortCallbackStore<TelegramApprovalTarget>,
}

impl TelegramApprovalStore {
    pub fn allocate_buttons(&mut self) -> TelegramApprovalButtons {
        TelegramApprovalButtons {
            key: self.approvals.reserve_key(),
        }
    }

    pub fn insert_target(
        &mut self,
        buttons: &TelegramApprovalButtons,
        session_id: String,
        approval: &ToolApprovalRequest,
        message: TelegramMessageHandle,
    ) {
        self.approvals.insert_reserved(
            buttons.key.clone(),
            TelegramApprovalTarget {
                session_id,
                approval_id: approval.approval_id.clone(),
                message,
            },
        );
    }

    pub fn resolve(&mut self, data: &str) -> Option<TelegramApprovalSelection> {
        let (outcome, key) = callback_outcome(data)?;
        let target = self.approvals.remove(key)?;
        Some(TelegramApprovalSelection { outcome, target })
    }

    pub fn lookup_callback(&mut self, data: &str) -> Option<TelegramApprovalSelection> {
        let (outcome, key) = callback_outcome(data)?;
        let target = self.approvals.get(key)?.clone();
        Some(TelegramApprovalSelection { outcome, target })
    }

    pub fn lookup_approval_id(
        &mut self,
        approval_id: &str,
        outcome: ToolPermissionOutcome,
    ) -> Option<TelegramApprovalSelection> {
        let target = self
            .approvals
            .find(|target| target.approval_id == approval_id)?
            .clone();
        Some(TelegramApprovalSelection { outcome, target })
    }

    pub fn resolve_approval_id(
        &mut self,
        approval_id: &str,
        outcome: ToolPermissionOutcome,
    ) -> Option<TelegramApprovalSelection> {
        let target = self
            .approvals
            .remove_where(|target| target.approval_id == approval_id)?;
        Some(TelegramApprovalSelection { outcome, target })
    }
}

pub fn render_pending_approval_requests(
    approvals: &BTreeMap<String, ToolApprovalRequest>,
    catalog: TelegramUiCatalog,
) -> String {
    if approvals.is_empty() {
        return catalog.pending_approvals_empty().into();
    }

    let mut text = catalog.pending_approvals_title(approvals.len());
    for (index, (approval_id, approval)) in approvals
        .iter()
        .take(PENDING_APPROVAL_RENDER_LIMIT)
        .enumerate()
    {
        text.push('\n');
        text.push_str(&catalog.pending_approval_item(
            index + 1,
            &approval.tool_call.name,
            approval_id,
        ));
    }

    let remaining = approvals
        .len()
        .saturating_sub(PENDING_APPROVAL_RENDER_LIMIT);
    if remaining > 0 {
        text.push('\n');
        text.push_str(&catalog.pending_approvals_more(remaining));
    }
    text
}

pub fn render_approval_request(
    approval: &ToolApprovalRequest,
    catalog: TelegramUiCatalog,
) -> String {
    let mut lines = vec![catalog.approval_request_title(&approval.tool_call.name)];
    if let Some(prompt) = approval
        .request
        .prompt
        .as_deref()
        .filter(|prompt| should_render_approval_prompt(approval, prompt))
    {
        lines.push(prompt.trim().into());
    }
    let arguments = render_arguments(&approval.tool_call.name, &approval.tool_call.arguments);
    if !arguments.is_empty() {
        lines.push(catalog.approval_arguments(&arguments));
    }
    if let Some(reason) = &approval.request.reason {
        lines.push(catalog.approval_reason(reason));
    }
    let permissions = render_permissions(&approval.permissions);
    if !permissions.is_empty() {
        lines.push(catalog.approval_permissions(&permissions));
    }
    if let Some(expires_at_ms) = approval.request.expires_at_ms {
        lines.push(catalog.approval_expires_at(expires_at_ms));
    }
    lines.join("\n")
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
        .map(render_permission)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_permission(permission: &ToolPermissionRequirement) -> String {
    match &permission.description {
        Some(description) if !description.trim().is_empty() => {
            format!(
                "{} - {}",
                permission.capability.as_str(),
                description.trim()
            )
        }
        _ => permission.capability.as_str().into(),
    }
}

fn should_render_approval_prompt(approval: &ToolApprovalRequest, prompt: &str) -> bool {
    let prompt = prompt.trim();
    !prompt.is_empty() && !prompt_contains_tool_call(prompt, &approval.tool_call)
}

fn prompt_contains_tool_call(prompt: &str, tool_call: &ToolCall) -> bool {
    let prompt = prompt.trim();
    let english_marker = format!("Tool: `{}`. Arguments:", tool_call.name);
    let chinese_marker = format!("工具：`{}`。参数：", tool_call.name);
    (prompt.starts_with("Review whether this tool call should be allowed.")
        && prompt.contains(&english_marker))
        || (prompt.starts_with("判断这个工具调用是否应该被允许。")
            && prompt.contains(&chinese_marker))
}

fn render_arguments(tool_name: &str, arguments: &Value) -> String {
    if is_host_exec_start_tool(tool_name)
        && let Some(summary) = render_host_exec_start_arguments(arguments)
    {
        return summary;
    }

    match arguments {
        Value::Object(arguments) => render_argument_object(arguments),
        _ => truncate_middle_chars(&arguments.to_string(), APPROVAL_ARGUMENT_RENDER_LIMIT),
    }
}

fn is_host_exec_start_tool(tool_name: &str) -> bool {
    tool_name == BuiltInToolName::HostExecStart.as_str() || tool_name == "host_exec_start"
}

fn render_host_exec_start_arguments(arguments: &Value) -> Option<String> {
    let input = parse_host_exec_start_arguments(arguments)?;
    let mut fields = vec![format!(
        "command={}",
        render_argument_string(&input.command, APPROVAL_ARGUMENT_VALUE_LIMIT)
    )];
    if let Some(shell) = input
        .shell
        .as_deref()
        .filter(|shell| !shell.trim().is_empty())
    {
        fields.push(format!(
            "shell={}",
            render_argument_string(shell, APPROVAL_ARGUMENT_VALUE_LIMIT)
        ));
    }
    if let Some(cwd) = input.cwd.as_ref() {
        fields.push(format!(
            "cwd={}",
            render_argument_string(&cwd.display().to_string(), APPROVAL_ARGUMENT_VALUE_LIMIT)
        ));
    }
    if !input.env.is_empty() {
        fields.push(format!("env={} keys", input.env.len()));
    }
    if input.pipe_stdin {
        fields.push("pipeStdin=true".into());
    }
    if let Some(max_spool_bytes) = input.max_spool_bytes {
        fields.push(format!("maxSpoolBytes={max_spool_bytes}"));
    }
    if let Some(foreground_wait_ms) = input.foreground_wait_ms {
        fields.push(format!("foregroundWaitMs={foreground_wait_ms}"));
    }
    Some(truncate_middle_chars(
        &fields.join(", "),
        APPROVAL_ARGUMENT_RENDER_LIMIT,
    ))
}

fn parse_host_exec_start_arguments(arguments: &Value) -> Option<StartCommandRequest> {
    parse_start_command(arguments)
        .or_else(|| parse_nested_start_command(arguments, "input"))
        .or_else(|| parse_nested_start_command(arguments, "arguments"))
}

fn parse_nested_start_command(arguments: &Value, key: &str) -> Option<StartCommandRequest> {
    parse_start_command(arguments.get(key)?)
}

fn parse_start_command(value: &Value) -> Option<StartCommandRequest> {
    serde_json::from_value(value.clone()).ok()
}

fn render_argument_object(arguments: &serde_json::Map<String, Value>) -> String {
    if arguments.is_empty() {
        return "{}".into();
    }

    let mut text = String::from("{");
    for (index, (key, value)) in arguments
        .iter()
        .take(APPROVAL_ARGUMENT_OBJECT_KEYS)
        .enumerate()
    {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&Value::String(key.clone()).to_string());
        text.push_str(": ");
        text.push_str(&render_argument_value_summary(value));
    }
    let remaining = arguments
        .len()
        .saturating_sub(APPROVAL_ARGUMENT_OBJECT_KEYS);
    if remaining > 0 {
        text.push_str(", ");
        text.push_str(&format!("... {remaining} more"));
    }
    text.push('}');

    truncate_middle_chars(&text, APPROVAL_ARGUMENT_RENDER_LIMIT)
}

fn render_argument_value_summary(value: &Value) -> String {
    match value {
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(map) => format!("{{{} keys}}", map.len()),
        Value::String(text) => render_argument_string(text, APPROVAL_ARGUMENT_VALUE_LIMIT),
        Value::Bool(_) | Value::Number(_) | Value::Null => value.to_string(),
    }
}

fn render_argument_string(text: &str, limit: usize) -> String {
    Value::String(truncate_end_chars(text, limit)).to_string()
}

#[cfg(test)]
mod tests {
    use super::{TelegramApprovalStore, render_approval_request, render_pending_approval_requests};
    use crate::i18n::TelegramUiCatalog;
    use crate::telegram_api::TelegramMessageHandle;
    use noloong_agent::Locale;
    use noloong_agent_core::{
        ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall, ToolPermissionOutcome,
        ToolPermissionRequirement,
    };
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;

    #[test]
    fn approval_button_allow_uses_short_callback_data() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        let approval = approval_request("approval-id-that-can-be-long");
        store.insert_target(
            &buttons,
            "session-1".into(),
            &approval,
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
        let approval = approval_request("approval-1");
        store.insert_target(
            &buttons,
            "session-1".into(),
            &approval,
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
        let mut approval = approval_request("approval-1");
        approval.permissions[0].description = Some("execute commands".into());
        approval.request.expires_at_ms = Some(1_700_000_000_000);

        let text = render_approval_request(&approval, TelegramUiCatalog::new(Locale::En));

        assert_eq!(
            text,
            "Approval required for `host_exec`\nRun command?\nArguments: {\"cmd\": \"ls\"}\nReason: User requested it\nPermissions: host.exec - execute commands\nExpires at: 1700000000000 ms"
        );
    }

    #[test]
    fn approval_message_summarizes_host_exec_start_without_duplicate_wrappers() {
        let command = "sh -c 'printf telegram-smoke-start; sleep 60; printf telegram-smoke-done'";
        let mut approval = approval_request("approval-1");
        approval.tool_call.name = "host.exec.start".into();
        approval.tool_call.arguments = json!({
            "command": command,
            "input": {"command": command},
            "arguments": {"command": command},
            "shell": "sh",
            "foregroundWaitMs": 1000
        });
        approval.request.prompt = Some(format!(
            "Review whether this tool call should be allowed. Tool: `{}`. Arguments: {}",
            approval.tool_call.name,
            serde_json::to_string_pretty(&approval.tool_call.arguments).unwrap()
        ));

        let text = render_approval_request(&approval, TelegramUiCatalog::new(Locale::En));

        assert_eq!(text.matches("telegram-smoke-start").count(), 1);
        assert!(text.contains("command=\"sh -c 'printf telegram-smoke-start;"));
        assert!(text.contains("shell=\"sh\""));
        assert!(text.contains("foregroundWaitMs=1000"));
        assert!(!text.contains("\"input\""));
        assert!(!text.contains("\"arguments\""));
    }

    #[test]
    fn approval_buttons_render_configured_locale() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();

        let markup = buttons.markup(TelegramUiCatalog::new(Locale::Zh));

        assert_eq!(markup.inline_keyboard[0][0].text, "允许");
        assert_eq!(markup.inline_keyboard[0][1].text, "拒绝");
    }

    #[test]
    fn approval_store_consumes_callback_once() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        let approval = approval_request("approval-1");
        store.insert_target(
            &buttons,
            "session-1".into(),
            &approval,
            TelegramMessageHandle {
                chat_id: 42,
                message_id: 9,
            },
        );

        let data = &buttons
            .markup(TelegramUiCatalog::new(Locale::En))
            .inline_keyboard[0][0]
            .callback_data;
        assert!(store.resolve(data).is_some());
        assert!(store.resolve(data).is_none());
    }

    #[test]
    fn approval_store_can_resolve_by_approval_id() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        let approval = approval_request("approval-1");
        store.insert_target(
            &buttons,
            "session-1".into(),
            &approval,
            TelegramMessageHandle {
                chat_id: 42,
                message_id: 9,
            },
        );

        let selection = store
            .resolve_approval_id("approval-1", ToolPermissionOutcome::Allow)
            .unwrap();
        let callback_data = &buttons
            .markup(TelegramUiCatalog::new(Locale::En))
            .inline_keyboard[0][0]
            .callback_data;

        assert_eq!(selection.outcome, ToolPermissionOutcome::Allow);
        assert_eq!(selection.target.approval_id, "approval-1");
        assert!(store.resolve(callback_data).is_none());
    }

    #[test]
    fn approval_store_lookup_does_not_consume_callback() {
        let mut store = TelegramApprovalStore::default();
        let buttons = store.allocate_buttons();
        let approval = approval_request("approval-1");
        store.insert_target(
            &buttons,
            "session-1".into(),
            &approval,
            TelegramMessageHandle {
                chat_id: 42,
                message_id: 9,
            },
        );
        let callback_data = &buttons
            .markup(TelegramUiCatalog::new(Locale::En))
            .inline_keyboard[0][0]
            .callback_data;

        assert!(store.lookup_callback(callback_data).is_some());
        assert!(
            store
                .lookup_approval_id("approval-1", ToolPermissionOutcome::Allow)
                .is_some()
        );
        assert!(store.resolve(callback_data).is_some());
    }

    #[test]
    fn pending_approval_requests_render_from_request_source() {
        let approvals = BTreeMap::from([("approval-1".into(), approval_request("approval-1"))]);

        let text = render_pending_approval_requests(&approvals, TelegramUiCatalog::new(Locale::En));

        assert_eq!(text, "Pending approvals: 1\n1. `host_exec` (approval-1)");
    }

    fn approval_request(approval_id: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            approval_id: approval_id.into(),
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
        }
    }
}
