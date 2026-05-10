use crate::commands::TelegramCockpitCommand;
use noloong_agent::Locale;
use noloong_agent_core::ToolPermissionOutcome;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelegramUiCatalog {
    locale: Locale,
}

impl TelegramUiCatalog {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn locale(self) -> Locale {
        self.locale
    }

    pub fn approval_allow_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Allow",
            Locale::Zh => "允许",
        }
    }

    pub fn approval_deny_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Deny",
            Locale::Zh => "拒绝",
        }
    }

    pub fn approval_resolution_reason(self) -> &'static str {
        match self.locale {
            Locale::En => "Resolved from Telegram inline approval",
            Locale::Zh => "通过 Telegram 行内审批处理",
        }
    }

    pub fn approval_request_title(self, tool_name: &str) -> String {
        match self.locale {
            Locale::En => format!("Approval required for `{tool_name}`"),
            Locale::Zh => format!("需要审批工具 `{tool_name}`"),
        }
    }

    pub fn approval_reason(self, reason: &str) -> String {
        match self.locale {
            Locale::En => format!("Reason: {reason}"),
            Locale::Zh => format!("原因：{reason}"),
        }
    }

    pub fn approval_arguments(self, arguments: &str) -> String {
        match self.locale {
            Locale::En => format!("Arguments: {arguments}"),
            Locale::Zh => format!("参数：{arguments}"),
        }
    }

    pub fn approval_permissions(self, permissions: &str) -> String {
        match self.locale {
            Locale::En => format!("Permissions: {permissions}"),
            Locale::Zh => format!("权限：{permissions}"),
        }
    }

    pub fn approval_expires_at(self, expires_at_ms: u64) -> String {
        match self.locale {
            Locale::En => format!("Expires at: {expires_at_ms} ms"),
            Locale::Zh => format!("过期时间：{expires_at_ms} 毫秒"),
        }
    }

    pub fn pending_approvals_title(self, count: usize) -> String {
        match self.locale {
            Locale::En => format!("Pending approvals: {count}"),
            Locale::Zh => format!("待处理审批：{count}"),
        }
    }

    pub fn pending_approvals_empty(self) -> &'static str {
        match self.locale {
            Locale::En => "No pending approvals",
            Locale::Zh => "没有待处理审批",
        }
    }

    pub fn pending_approval_item(self, index: usize, tool_name: &str, approval_id: &str) -> String {
        match self.locale {
            Locale::En => format!("{index}. `{tool_name}` ({approval_id})"),
            Locale::Zh => format!("{index}. `{tool_name}`（{approval_id}）"),
        }
    }

    pub fn pending_approvals_more(self, remaining: usize) -> String {
        match self.locale {
            Locale::En => format!("... and {remaining} more"),
            Locale::Zh => format!("... 另有 {remaining} 个"),
        }
    }

    pub fn command_description(self, command: TelegramCockpitCommand) -> &'static str {
        match (self.locale, command) {
            (Locale::En, TelegramCockpitCommand::Start) => "Open the Noloong cockpit",
            (Locale::En, TelegramCockpitCommand::Help) => "Show command help",
            (Locale::En, TelegramCockpitCommand::Status) => "Show active session status",
            (Locale::En, TelegramCockpitCommand::New) => "Start a new session",
            (Locale::En, TelegramCockpitCommand::Switch) => "Switch active session",
            (Locale::En, TelegramCockpitCommand::Sessions) => "List chat sessions",
            (Locale::En, TelegramCockpitCommand::Profiles) => "List runtime profiles",
            (Locale::En, TelegramCockpitCommand::Continue) => "Continue the active run",
            (Locale::En, TelegramCockpitCommand::Abort) => "Abort the active run",
            (Locale::En, TelegramCockpitCommand::Queue) => "Inspect message queues",
            (Locale::En, TelegramCockpitCommand::Approvals) => "List pending approvals",
            (Locale::En, TelegramCockpitCommand::Processes) => "List background processes",
            (Locale::En, TelegramCockpitCommand::Process) => "Inspect one background process",
            (Locale::En, TelegramCockpitCommand::Manifest) => "Inspect manifest and proposals",
            (Locale::En, TelegramCockpitCommand::Subagent) => "Spawn a subagent session",
            (Locale::En, TelegramCockpitCommand::Settings) => "Show bridge settings",
            (Locale::Zh, TelegramCockpitCommand::Start) => "打开 Noloong 控制台",
            (Locale::Zh, TelegramCockpitCommand::Help) => "显示命令帮助",
            (Locale::Zh, TelegramCockpitCommand::Status) => "查看当前会话状态",
            (Locale::Zh, TelegramCockpitCommand::New) => "开始新会话",
            (Locale::Zh, TelegramCockpitCommand::Switch) => "切换当前会话",
            (Locale::Zh, TelegramCockpitCommand::Sessions) => "列出聊天会话",
            (Locale::Zh, TelegramCockpitCommand::Profiles) => "列出运行配置",
            (Locale::Zh, TelegramCockpitCommand::Continue) => "继续当前运行",
            (Locale::Zh, TelegramCockpitCommand::Abort) => "中止当前运行",
            (Locale::Zh, TelegramCockpitCommand::Queue) => "查看消息队列",
            (Locale::Zh, TelegramCockpitCommand::Approvals) => "列出待处理审批",
            (Locale::Zh, TelegramCockpitCommand::Processes) => "列出后台进程",
            (Locale::Zh, TelegramCockpitCommand::Process) => "查看单个后台进程",
            (Locale::Zh, TelegramCockpitCommand::Manifest) => "查看 manifest 与提案",
            (Locale::Zh, TelegramCockpitCommand::Subagent) => "创建子智能体会话",
            (Locale::Zh, TelegramCockpitCommand::Settings) => "查看桥接设置",
        }
    }

    pub fn command_help_title(self) -> &'static str {
        match self.locale {
            Locale::En => "Noloong cockpit commands:",
            Locale::Zh => "Noloong 控制台命令：",
        }
    }

    pub fn command_help_item(self, command: TelegramCockpitCommand) -> String {
        format!(
            "/{} - {}",
            command.name(),
            self.command_description(command)
        )
    }

    pub fn unknown_command(self, name: &str) -> String {
        match self.locale {
            Locale::En => format!("Unknown command: /{name}"),
            Locale::Zh => format!("未知命令：/{name}"),
        }
    }

    pub fn command_not_ready(self, command: TelegramCockpitCommand) -> String {
        match self.locale {
            Locale::En => format!(
                "/{} is in the cockpit menu. Its control surface is not implemented yet.",
                command.name()
            ),
            Locale::Zh => format!(
                "/{} 已在控制台菜单中，但对应控制面尚未实现。",
                command.name()
            ),
        }
    }

    pub fn callback_not_allowed(self) -> &'static str {
        match self.locale {
            Locale::En => "Not allowed",
            Locale::Zh => "无权操作",
        }
    }

    pub fn callback_approval_expired(self) -> &'static str {
        match self.locale {
            Locale::En => "Approval expired",
            Locale::Zh => "审批已过期",
        }
    }

    pub fn callback_recorded(self) -> &'static str {
        match self.locale {
            Locale::En => "Recorded",
            Locale::Zh => "已记录",
        }
    }

    pub fn approval_resolved(self, outcome: &ToolPermissionOutcome) -> String {
        match self.locale {
            Locale::En => format!("Approval resolved: {}", self.approval_outcome(outcome)),
            Locale::Zh => format!("审批已处理：{}", self.approval_outcome(outcome)),
        }
    }

    pub fn tool_started(self, tool_name: &str) -> String {
        match self.locale {
            Locale::En => format!("Tool started: {tool_name}"),
            Locale::Zh => format!("工具已开始：{tool_name}"),
        }
    }

    pub fn tool_completed(self, tool_call_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Tool completed: {tool_call_id}"),
            Locale::Zh => format!("工具已完成：{tool_call_id}"),
        }
    }

    pub fn run_failed(self, run_id: &str, error: &str) -> String {
        match self.locale {
            Locale::En => format!("Run failed\nRun: {run_id}\nError: {error}"),
            Locale::Zh => format!("运行失败\n运行：{run_id}\n错误：{error}"),
        }
    }

    pub fn run_started(self, run_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Run started\nRun: {run_id}"),
            Locale::Zh => format!("运行已开始\n运行：{run_id}"),
        }
    }

    pub fn run_completed(self, run_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Run completed\nRun: {run_id}"),
            Locale::Zh => format!("运行已完成\n运行：{run_id}"),
        }
    }

    pub fn run_paused(self, run_id: &str, reason: &Value) -> String {
        let reason = self.run_pause_reason(reason);
        match self.locale {
            Locale::En => format!("Run paused\nRun: {run_id}\nReason: {reason}"),
            Locale::Zh => format!("运行已暂停\n运行：{run_id}\n原因：{reason}"),
        }
    }

    pub fn media_input_failed(self, error: &str) -> String {
        match self.locale {
            Locale::En => format!("Media input failed: {error}"),
            Locale::Zh => format!("媒体输入失败：{error}"),
        }
    }

    fn approval_outcome(self, outcome: &ToolPermissionOutcome) -> &'static str {
        match (self.locale, outcome) {
            (Locale::En, ToolPermissionOutcome::Allow) => "allow",
            (Locale::En, ToolPermissionOutcome::Deny) => "deny",
            (Locale::Zh, ToolPermissionOutcome::Allow) => "允许",
            (Locale::Zh, ToolPermissionOutcome::Deny) => "拒绝",
        }
    }

    fn run_pause_reason(self, reason: &Value) -> String {
        if let Some(reason) = reason.as_str() {
            return reason.into();
        }
        match reason.get("type").and_then(Value::as_str) {
            Some("tool_approval") => match self.locale {
                Locale::En => "tool approval required".into(),
                Locale::Zh => "需要工具审批".into(),
            },
            Some(kind) => match self.locale {
                Locale::En => format!("paused by {kind}"),
                Locale::Zh => format!("由 {kind} 暂停"),
            },
            None if reason.is_null() => match self.locale {
                Locale::En => "unknown",
                Locale::Zh => "未知",
            }
            .into(),
            None => match self.locale {
                Locale::En => "runtime requested a pause".into(),
                Locale::Zh => "运行时请求暂停".into(),
            },
        }
    }
}

impl Default for TelegramUiCatalog {
    fn default() -> Self {
        Self::new(Locale::En)
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramUiCatalog;
    use crate::commands::TelegramCockpitCommand;
    use noloong_agent::Locale;
    use noloong_agent_core::ToolPermissionOutcome;

    #[test]
    fn ui_catalog_localizes_approval_buttons() {
        let catalog = TelegramUiCatalog::new(Locale::Zh);

        assert_eq!(catalog.approval_allow_button(), "允许");
        assert_eq!(catalog.approval_deny_button(), "拒绝");
    }

    #[test]
    fn ui_catalog_localizes_approval_resolution() {
        let catalog = TelegramUiCatalog::new(Locale::Zh);

        assert_eq!(
            catalog.approval_resolved(&ToolPermissionOutcome::Allow),
            "审批已处理：允许"
        );
    }

    #[test]
    fn ui_catalog_localizes_command_descriptions() {
        let catalog = TelegramUiCatalog::new(Locale::Zh);

        assert_eq!(
            catalog.command_description(TelegramCockpitCommand::Approvals),
            "列出待处理审批"
        );
    }
}
