use crate::{
    commands::TelegramCockpitCommand,
    queue::{
        TelegramQueueKind, TelegramQueueSnapshot, TelegramQueueSummaryLabels,
        TelegramQueuedMessageIntent,
    },
};
use noloong_agent::{Locale, interaction::InteractionSessionStatus};
use noloong_agent_core::{QueueMode, ToolPermissionOutcome};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelegramUiCatalog {
    locale: Locale,
}

pub struct TelegramStatusCard<'a> {
    pub session_id: &'a str,
    pub profile_id: &'a str,
    pub status: &'a InteractionSessionStatus,
    pub messages: usize,
    pub tools: usize,
    pub pending_approvals: usize,
    pub plugins: usize,
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

    pub fn profile_list_title(self, count: usize) -> String {
        match self.locale {
            Locale::En => format!("Profiles: {count}"),
            Locale::Zh => format!("运行配置：{count}"),
        }
    }

    pub fn profile_item(self, index: usize, display_name: &str, profile_id: &str) -> String {
        match self.locale {
            Locale::En => format!("{index}. {display_name} ({profile_id})"),
            Locale::Zh => format!("{index}. {display_name}（{profile_id}）"),
        }
    }

    pub fn profile_selected(self, profile_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Default profile selected: {profile_id}"),
            Locale::Zh => format!("已选择默认运行配置：{profile_id}"),
        }
    }

    pub fn select_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Select",
            Locale::Zh => "选择",
        }
    }

    pub fn session_list_title(self, count: usize) -> String {
        match self.locale {
            Locale::En => format!("Sessions: {count}"),
            Locale::Zh => format!("会话：{count}"),
        }
    }

    pub fn no_sessions(self) -> &'static str {
        match self.locale {
            Locale::En => "No Telegram sessions yet",
            Locale::Zh => "还没有 Telegram 会话",
        }
    }

    pub fn no_active_session(self) -> &'static str {
        match self.locale {
            Locale::En => "No active session",
            Locale::Zh => "没有当前会话",
        }
    }

    pub fn session_item(
        self,
        index: usize,
        session_id: &str,
        profile_id: &str,
        status: &InteractionSessionStatus,
        active: bool,
    ) -> String {
        let marker = if active { " *" } else { "" };
        let status = self.session_status(status);
        match self.locale {
            Locale::En => {
                format!("{index}. {session_id}{marker}\nProfile: {profile_id}\nStatus: {status}")
            }
            Locale::Zh => {
                format!("{index}. {session_id}{marker}\n配置：{profile_id}\n状态：{status}")
            }
        }
    }

    pub fn session_created(self, session_id: &str, profile_id: &str) -> String {
        match self.locale {
            Locale::En => format!("New active session: {session_id}\nProfile: {profile_id}"),
            Locale::Zh => format!("新的当前会话：{session_id}\n配置：{profile_id}"),
        }
    }

    pub fn session_switched(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Active session switched: {session_id}"),
            Locale::Zh => format!("已切换当前会话：{session_id}"),
        }
    }

    pub fn session_delete_confirm(self, session_id: &str, force_abort: bool) -> String {
        match (self.locale, force_abort) {
            (Locale::En, true) => {
                format!("Delete running session and force abort?\nSession: {session_id}")
            }
            (Locale::En, false) => format!("Delete session?\nSession: {session_id}"),
            (Locale::Zh, true) => format!("删除运行中的会话并强制中止？\n会话：{session_id}"),
            (Locale::Zh, false) => format!("删除会话？\n会话：{session_id}"),
        }
    }

    pub fn session_deleted(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Session deleted: {session_id}"),
            Locale::Zh => format!("已删除会话：{session_id}"),
        }
    }

    pub fn switch_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Switch",
            Locale::Zh => "切换",
        }
    }

    pub fn delete_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Delete",
            Locale::Zh => "删除",
        }
    }

    pub fn confirm_delete_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Confirm delete",
            Locale::Zh => "确认删除",
        }
    }

    pub fn run_continued(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Run continued\nSession: {session_id}"),
            Locale::Zh => format!("已继续运行\n会话：{session_id}"),
        }
    }

    pub fn run_abort_confirm(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Abort running session?\nSession: {session_id}"),
            Locale::Zh => format!("中止运行中的会话？\n会话：{session_id}"),
        }
    }

    pub fn run_aborted(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Run aborted\nSession: {session_id}"),
            Locale::Zh => format!("运行已中止\n会话：{session_id}"),
        }
    }

    pub fn confirm_abort_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Confirm abort",
            Locale::Zh => "确认中止",
        }
    }

    pub fn queue_follow_up_added(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Follow-up queued\nSession: {session_id}"),
            Locale::Zh => format!("已加入后续输入队列\n会话：{session_id}"),
        }
    }

    pub fn queue_card(self, snapshot: &TelegramQueueSnapshot) -> String {
        let mut lines = vec![self.queue_title(snapshot)];
        self.push_queue_section(&mut lines, TelegramQueueKind::Steering, &snapshot.steering);
        self.push_queue_section(&mut lines, TelegramQueueKind::FollowUp, &snapshot.follow_up);
        lines.join("\n")
    }

    pub fn queue_cleared(self, queue: TelegramQueueKind, remaining: usize) -> String {
        match self.locale {
            Locale::En => format!(
                "{} queue cleared\nRemaining: {remaining}",
                self.queue_kind_label(queue)
            ),
            Locale::Zh => format!(
                "{}队列已清空\n剩余：{remaining}",
                self.queue_kind_label(queue)
            ),
        }
    }

    pub fn queue_mode_updated(
        self,
        queue: TelegramQueueKind,
        mode: QueueMode,
        messages: usize,
    ) -> String {
        match self.locale {
            Locale::En => format!(
                "{} queue mode: {}\nMessages: {messages}",
                self.queue_kind_label(queue),
                self.queue_mode_label(mode)
            ),
            Locale::Zh => format!(
                "{}队列模式：{}\n消息：{messages}",
                self.queue_kind_label(queue),
                self.queue_mode_label(mode)
            ),
        }
    }

    pub fn clear_queue_button(self, queue: TelegramQueueKind) -> String {
        match self.locale {
            Locale::En => format!("Clear {}", self.queue_kind_label(queue)),
            Locale::Zh => format!("清空{}", self.queue_kind_label(queue)),
        }
    }

    pub fn set_queue_mode_button(self, queue: TelegramQueueKind, mode: QueueMode) -> String {
        match self.locale {
            Locale::En => format!(
                "{}: {}",
                self.queue_kind_label(queue),
                self.queue_mode_label(mode)
            ),
            Locale::Zh => format!(
                "{}：{}",
                self.queue_kind_label(queue),
                self.queue_mode_label(mode)
            ),
        }
    }

    pub fn status_card(self, card: TelegramStatusCard<'_>) -> String {
        let status = self.session_status(card.status);
        match self.locale {
            Locale::En => format!(
                "Active session\nSession: {}\nProfile: {}\nStatus: {status}\nMessages: {}\nTools: {}\nPending approvals: {}\nPlugins: {}",
                card.session_id,
                card.profile_id,
                card.messages,
                card.tools,
                card.pending_approvals,
                card.plugins
            ),
            Locale::Zh => format!(
                "当前会话\n会话：{}\n配置：{}\n状态：{status}\n消息：{}\n工具：{}\n待审批：{}\n插件：{}",
                card.session_id,
                card.profile_id,
                card.messages,
                card.tools,
                card.pending_approvals,
                card.plugins
            ),
        }
    }

    pub fn queue_kind_label(self, queue: TelegramQueueKind) -> &'static str {
        match (self.locale, queue) {
            (Locale::En, TelegramQueueKind::Steering) => "Steering",
            (Locale::En, TelegramQueueKind::FollowUp) => "Follow-up",
            (Locale::Zh, TelegramQueueKind::Steering) => "引导",
            (Locale::Zh, TelegramQueueKind::FollowUp) => "后续输入",
        }
    }

    pub fn queue_mode_label(self, mode: QueueMode) -> &'static str {
        match (self.locale, mode) {
            (Locale::En, QueueMode::All) => "all",
            (Locale::En, QueueMode::OneAtATime) => "one at a time",
            (Locale::Zh, QueueMode::All) => "全部",
            (Locale::Zh, QueueMode::OneAtATime) => "逐条",
        }
    }

    pub fn session_status(self, status: &InteractionSessionStatus) -> &'static str {
        match (self.locale, status) {
            (Locale::En, InteractionSessionStatus::Idle) => "idle",
            (Locale::En, InteractionSessionStatus::Running) => "running",
            (Locale::En, InteractionSessionStatus::Completed) => "completed",
            (Locale::En, InteractionSessionStatus::Aborted) => "aborted",
            (Locale::En, InteractionSessionStatus::Failed) => "failed",
            (Locale::En, InteractionSessionStatus::Paused) => "paused",
            (Locale::Zh, InteractionSessionStatus::Idle) => "空闲",
            (Locale::Zh, InteractionSessionStatus::Running) => "运行中",
            (Locale::Zh, InteractionSessionStatus::Completed) => "已完成",
            (Locale::Zh, InteractionSessionStatus::Aborted) => "已中止",
            (Locale::Zh, InteractionSessionStatus::Failed) => "失败",
            (Locale::Zh, InteractionSessionStatus::Paused) => "已暂停",
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

    pub fn callback_action_expired(self) -> &'static str {
        match self.locale {
            Locale::En => "Action expired",
            Locale::Zh => "操作已过期",
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

    fn queue_title(self, snapshot: &TelegramQueueSnapshot) -> String {
        let total = snapshot.steering.len() + snapshot.follow_up.len();
        match self.locale {
            Locale::En => format!("Queues: {total}"),
            Locale::Zh => format!("队列：{total}"),
        }
    }

    fn push_queue_section(
        self,
        lines: &mut Vec<String>,
        queue: TelegramQueueKind,
        messages: &[crate::queue::TelegramQueuedMessage],
    ) {
        lines.push(match self.locale {
            Locale::En => format!("{}: {}", self.queue_kind_label(queue), messages.len()),
            Locale::Zh => format!("{}：{}", self.queue_kind_label(queue), messages.len()),
        });
        if messages.is_empty() {
            lines.push(match self.locale {
                Locale::En => "  empty".into(),
                Locale::Zh => "  空".into(),
            });
            return;
        }
        for (index, message) in messages.iter().take(5).enumerate() {
            lines.push(self.queue_item(index + 1, message));
        }
        let remaining = messages.len().saturating_sub(5);
        if remaining > 0 {
            lines.push(match self.locale {
                Locale::En => format!("  ... and {remaining} more"),
                Locale::Zh => format!("  ... 另有 {remaining} 条"),
            });
        }
    }

    fn queue_item(self, index: usize, message: &crate::queue::TelegramQueuedMessage) -> String {
        let intent = match (self.locale, message.intent) {
            (Locale::En, TelegramQueuedMessageIntent::Observation) => "observation",
            (Locale::En, TelegramQueuedMessageIntent::UserInput) => "user input",
            (Locale::Zh, TelegramQueuedMessageIntent::Observation) => "观察",
            (Locale::Zh, TelegramQueuedMessageIntent::UserInput) => "用户输入",
        };
        format!(
            "  {index}. {intent}: {}",
            crate::queue::summarize_queued_message(message, self.queue_summary_labels())
        )
    }

    fn queue_summary_labels(self) -> TelegramQueueSummaryLabels<'static> {
        match self.locale {
            Locale::En => TelegramQueueSummaryLabels {
                non_text_message: "[non-text message]",
                json: "[json]",
                file: "file",
                image: "image",
                audio: "audio",
                video: "video",
            },
            Locale::Zh => TelegramQueueSummaryLabels {
                non_text_message: "[非文本消息]",
                json: "[JSON]",
                file: "文件",
                image: "图片",
                audio: "音频",
                video: "视频",
            },
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
