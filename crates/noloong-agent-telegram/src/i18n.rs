use crate::{
    commands::TelegramCockpitCommand,
    media::{TelegramMediaFallbackKind, TelegramMediaFallbackNotice, TelegramMediaResolutionError},
    queue::{
        TelegramQueueKind, TelegramQueueSnapshot, TelegramQueueSummaryLabels,
        TelegramQueuedMessageIntent,
    },
    text::whitespace_prefix_summary,
};
use noloong_agent::{
    AgentManifest, JobSnapshot, JobStatus, ManifestPatchProposal, ResolvedSystemPrompt, WaitOutcome,
};
use noloong_agent::{
    Locale,
    interaction::{InteractionSessionDescriptor, InteractionSessionStatus},
};
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

pub struct TelegramManifestCard<'a> {
    pub manifest: &'a AgentManifest,
    pub system_prompt: &'a ResolvedSystemPrompt,
    pub proposals: &'a [ManifestPatchProposal],
}

pub const MANIFEST_PROPOSAL_DISPLAY_LIMIT: usize = 5;

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
            (Locale::En, TelegramCockpitCommand::Approve) => "Approve a pending tool call",
            (Locale::En, TelegramCockpitCommand::Deny) => "Deny a pending tool call",
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
            (Locale::Zh, TelegramCockpitCommand::Approve) => "批准待处理工具调用",
            (Locale::Zh, TelegramCockpitCommand::Deny) => "拒绝待处理工具调用",
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

    pub fn process_usage(self) -> &'static str {
        match self.locale {
            Locale::En => "Usage: /process <job_id> [write <text>]",
            Locale::Zh => "用法：/process <job_id> [write <text>]",
        }
    }

    pub fn process_list_title(self, count: usize) -> String {
        match self.locale {
            Locale::En => format!("Processes: {count}"),
            Locale::Zh => format!("后台进程：{count}"),
        }
    }

    pub fn no_processes(self) -> &'static str {
        match self.locale {
            Locale::En => "No background processes",
            Locale::Zh => "没有后台进程",
        }
    }

    pub fn process_item(self, index: usize, snapshot: &JobSnapshot) -> String {
        match self.locale {
            Locale::En => format!(
                "{index}. `{}`\nCommand: `{}`\nStatus: {}",
                snapshot.job_id,
                snapshot.command,
                self.process_status(&snapshot.status)
            ),
            Locale::Zh => format!(
                "{index}. `{}`\n命令：`{}`\n状态：{}",
                snapshot.job_id,
                snapshot.command,
                self.process_status(&snapshot.status)
            ),
        }
    }

    pub fn process_output_card(
        self,
        job_id: &str,
        status: &JobStatus,
        output: &str,
        truncated: bool,
    ) -> String {
        let truncated = if truncated {
            match self.locale {
                Locale::En => "\nOutput truncated",
                Locale::Zh => "\n输出已截断",
            }
        } else {
            ""
        };
        match self.locale {
            Locale::En => format!(
                "Process `{job_id}`\nStatus: {}\n{output}{truncated}",
                self.process_status(status)
            ),
            Locale::Zh => format!(
                "进程 `{job_id}`\n状态：{}\n{output}{truncated}",
                self.process_status(status)
            ),
        }
    }

    pub fn process_output_attached(self, job_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Process output attached\nJob: `{job_id}`"),
            Locale::Zh => format!("进程输出已作为文件发送\n任务：`{job_id}`"),
        }
    }

    pub fn process_wait_result(self, outcome: &WaitOutcome) -> String {
        match self.locale {
            Locale::En => format!(
                "Wait result\nJob: `{}`\nStatus: {}\nTimed out: {}",
                outcome.job_id,
                self.process_status(&outcome.status),
                outcome.timed_out
            ),
            Locale::Zh => format!(
                "等待结果\n任务：`{}`\n状态：{}\n超时：{}",
                outcome.job_id,
                self.process_status(&outcome.status),
                outcome.timed_out
            ),
        }
    }

    pub fn process_terminate_confirm(self, job_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Terminate process?\nJob: `{job_id}`"),
            Locale::Zh => format!("终止进程？\n任务：`{job_id}`"),
        }
    }

    pub fn process_terminated(self, snapshot: &JobSnapshot) -> String {
        match self.locale {
            Locale::En => format!(
                "Process terminated\nJob: `{}`\nStatus: {}",
                snapshot.job_id,
                self.process_status(&snapshot.status)
            ),
            Locale::Zh => format!(
                "进程已终止\n任务：`{}`\n状态：{}",
                snapshot.job_id,
                self.process_status(&snapshot.status)
            ),
        }
    }

    pub fn process_write_confirm(self, job_id: &str, text: &str) -> String {
        match self.locale {
            Locale::En => format!("Write to process stdin?\nJob: `{job_id}`\nText: {text}"),
            Locale::Zh => format!("写入进程 stdin？\n任务：`{job_id}`\n文本：{text}"),
        }
    }

    pub fn process_written(self, snapshot: &JobSnapshot) -> String {
        match self.locale {
            Locale::En => format!(
                "Wrote to process\nJob: `{}`\nStatus: {}",
                snapshot.job_id,
                self.process_status(&snapshot.status)
            ),
            Locale::Zh => format!(
                "已写入进程\n任务：`{}`\n状态：{}",
                snapshot.job_id,
                self.process_status(&snapshot.status)
            ),
        }
    }

    pub fn open_process_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Open",
            Locale::Zh => "打开",
        }
    }

    pub fn read_process_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Read more",
            Locale::Zh => "继续读取",
        }
    }

    pub fn wait_process_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Wait",
            Locale::Zh => "等待",
        }
    }

    pub fn terminate_process_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Terminate",
            Locale::Zh => "终止",
        }
    }

    pub fn confirm_write_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Confirm write",
            Locale::Zh => "确认写入",
        }
    }

    pub fn manifest_card(self, card: TelegramManifestCard<'_>) -> String {
        let tools = card
            .manifest
            .enabled_tools
            .iter()
            .map(|tool| tool.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let tools = if tools.is_empty() {
            self.manifest_no_enabled_tools().to_owned()
        } else {
            tools
        };
        let prompt_summary = whitespace_prefix_summary(&card.system_prompt.effective_text, 260);
        let mut lines = match self.locale {
            Locale::En => vec![
                "Manifest".to_owned(),
                format!(
                    "System prompt: {}",
                    self.manifest_prompt_source(card.system_prompt)
                ),
                format!("Prompt summary: {prompt_summary}"),
                format!("Enabled tools: {tools}"),
                format!("Plugins: {}", card.manifest.plugins.len()),
                format!("Pending proposals: {}", card.proposals.len()),
            ],
            Locale::Zh => vec![
                "Manifest".to_owned(),
                format!(
                    "系统提示词：{}",
                    self.manifest_prompt_source(card.system_prompt)
                ),
                format!("提示词摘要：{prompt_summary}"),
                format!("已启用工具：{tools}"),
                format!("插件：{}", card.manifest.plugins.len()),
                format!("待处理提案：{}", card.proposals.len()),
            ],
        };
        if card.proposals.is_empty() {
            lines.push(self.manifest_no_pending_proposals().into());
        } else {
            for (index, proposal) in card
                .proposals
                .iter()
                .take(MANIFEST_PROPOSAL_DISPLAY_LIMIT)
                .enumerate()
            {
                lines.push(self.manifest_proposal_item(index + 1, proposal));
            }
            let remaining = card
                .proposals
                .len()
                .saturating_sub(MANIFEST_PROPOSAL_DISPLAY_LIMIT);
            if remaining > 0 {
                lines.push(match self.locale {
                    Locale::En => format!("... and {remaining} more proposals"),
                    Locale::Zh => format!("... 另有 {remaining} 个提案"),
                });
            }
        }
        lines.join("\n")
    }

    pub fn manifest_proposal_item(self, index: usize, proposal: &ManifestPatchProposal) -> String {
        match self.locale {
            Locale::En => format!("{index}. `{}`\n{}", proposal.proposal_id, proposal.summary),
            Locale::Zh => format!("{index}. `{}`\n{}", proposal.proposal_id, proposal.summary),
        }
    }

    pub fn manifest_proposal_approved(self, proposal: &ManifestPatchProposal) -> String {
        match self.locale {
            Locale::En => format!(
                "Manifest proposal approved\nProposal: `{}`\n{}",
                proposal.proposal_id, proposal.summary
            ),
            Locale::Zh => format!(
                "Manifest 提案已批准\n提案：`{}`\n{}",
                proposal.proposal_id, proposal.summary
            ),
        }
    }

    pub fn manifest_apply_confirm(self, session_id: &str) -> String {
        match self.locale {
            Locale::En => format!("Apply approved manifest proposals?\nSession: {session_id}"),
            Locale::Zh => format!("应用已批准的 manifest 提案？\n会话：{session_id}"),
        }
    }

    pub fn manifest_applied(self, applied_proposal_ids: &[String]) -> String {
        if applied_proposal_ids.is_empty() {
            return match self.locale {
                Locale::En => "No approved manifest proposals to apply".into(),
                Locale::Zh => "没有可应用的已批准 manifest 提案".into(),
            };
        }
        let ids = applied_proposal_ids
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ");
        match self.locale {
            Locale::En => format!("Manifest updated\nApplied: {ids}"),
            Locale::Zh => format!("Manifest 已更新\n已应用：{ids}"),
        }
    }

    pub fn approve_manifest_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Approve",
            Locale::Zh => "批准",
        }
    }

    pub fn apply_manifest_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Apply approved",
            Locale::Zh => "应用已批准",
        }
    }

    pub fn confirm_apply_manifest_button(self) -> &'static str {
        match self.locale {
            Locale::En => "Confirm apply",
            Locale::Zh => "确认应用",
        }
    }

    pub fn subagent_usage(self) -> &'static str {
        match self.locale {
            Locale::En => "Usage: /subagent <role> [initial prompt]",
            Locale::Zh => "用法：/subagent <role> [initial prompt]",
        }
    }

    pub fn subagent_spawned(self, descriptor: &InteractionSessionDescriptor) -> String {
        match self.locale {
            Locale::En => format!(
                "Subagent session created\nSession: {}\nRole: {}\nStatus: {}",
                descriptor.session_id,
                descriptor.role.as_deref().unwrap_or("subagent"),
                self.session_status(&descriptor.status)
            ),
            Locale::Zh => format!(
                "子智能体会话已创建\n会话：{}\n角色：{}\n状态：{}",
                descriptor.session_id,
                descriptor.role.as_deref().unwrap_or("subagent"),
                self.session_status(&descriptor.status)
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

    pub fn process_status(self, status: &JobStatus) -> String {
        match (self.locale, status) {
            (Locale::En, JobStatus::Running) => "running".into(),
            (Locale::En, JobStatus::Exited { code }) => format!("exited ({code:?})"),
            (Locale::En, JobStatus::Terminated) => "terminated".into(),
            (Locale::En, JobStatus::Failed { error }) => format!("failed ({error})"),
            (Locale::Zh, JobStatus::Running) => "运行中".into(),
            (Locale::Zh, JobStatus::Exited { code }) => format!("已退出（{code:?}）"),
            (Locale::Zh, JobStatus::Terminated) => "已终止".into(),
            (Locale::Zh, JobStatus::Failed { error }) => format!("失败（{error}）"),
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

    pub fn media_resolution_failed(self, error: &TelegramMediaResolutionError) -> String {
        self.media_input_failed(&self.media_resolution_error(error))
    }

    pub fn unsupported_media_fallback_notices(
        self,
        notices: &[TelegramMediaFallbackNotice],
    ) -> String {
        notices
            .iter()
            .map(|notice| self.unsupported_media_fallback_notice(notice))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn unsupported_media_fallback_notice(self, notice: &TelegramMediaFallbackNotice) -> String {
        let kind = self.media_fallback_kind(notice.original_kind);
        match self.locale {
            Locale::En => format!(
                "This model/provider cannot process {kind} attachments in native media mode ({}; {}). Submitted it as a regular file instead.",
                notice.file_name, notice.mime_type
            ),
            Locale::Zh => format!(
                "当前模型/接口暂不支持以原生媒体方式处理{kind}（{}；{}），已按普通文件提交。",
                notice.file_name, notice.mime_type
            ),
        }
    }

    pub fn input_submission_failed(self, error: &str) -> String {
        let error = whitespace_prefix_summary(error, 700);
        match self.locale {
            Locale::En => format!("Message could not be submitted to the agent: {error}"),
            Locale::Zh => format!("消息未能提交给智能体：{error}"),
        }
    }

    pub fn input_submission_still_running(self) -> &'static str {
        match self.locale {
            Locale::En => {
                "The agent is still running. I will deliver the result here when it finishes."
            }
            Locale::Zh => "智能体仍在运行；完成后我会把结果发到这里。",
        }
    }

    fn manifest_no_enabled_tools(self) -> &'static str {
        match self.locale {
            Locale::En => "none",
            Locale::Zh => "无",
        }
    }

    fn media_fallback_kind(self, kind: TelegramMediaFallbackKind) -> &'static str {
        match (self.locale, kind) {
            (Locale::En, TelegramMediaFallbackKind::Audio) => "audio",
            (Locale::En, TelegramMediaFallbackKind::Voice) => "voice",
            (Locale::En, TelegramMediaFallbackKind::Video) => "video",
            (Locale::Zh, TelegramMediaFallbackKind::Audio) => "音频",
            (Locale::Zh, TelegramMediaFallbackKind::Voice) => "语音",
            (Locale::Zh, TelegramMediaFallbackKind::Video) => "视频",
        }
    }

    fn manifest_no_pending_proposals(self) -> &'static str {
        match self.locale {
            Locale::En => "No pending manifest proposals",
            Locale::Zh => "没有待处理 manifest 提案",
        }
    }

    fn manifest_prompt_source(self, system_prompt: &ResolvedSystemPrompt) -> String {
        let source = system_prompt.source.as_str();
        let configured = system_prompt
            .configured_profile
            .map(|profile| profile.as_str())
            .unwrap_or("custom");
        let resolved = system_prompt
            .resolved_profile
            .map(|profile| profile.as_str())
            .unwrap_or(configured);
        match self.locale {
            Locale::En => format!("{source}, configured={configured}, resolved={resolved}"),
            Locale::Zh => format!("{source}，配置={configured}，解析={resolved}"),
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

    fn media_resolution_error(self, error: &TelegramMediaResolutionError) -> String {
        match error {
            TelegramMediaResolutionError::FileTooLarge {
                file_id,
                limit,
                actual,
            } => match self.locale {
                Locale::En => format!(
                    "file `{file_id}` is too large (limit: {limit} bytes, actual: {})",
                    self.media_file_size_label(*actual)
                ),
                Locale::Zh => format!(
                    "文件 `{file_id}` 过大（限制：{limit} 字节，实际：{}）",
                    self.media_file_size_label(*actual)
                ),
            },
            TelegramMediaResolutionError::MissingMime { file_id, kind } => match self.locale {
                Locale::En => format!(
                    "file `{file_id}` is missing or has an unknown MIME type for {}",
                    self.media_input_kind_label(kind)
                ),
                Locale::Zh => format!(
                    "文件 `{file_id}` 缺少或无法识别 {} 的 MIME 类型",
                    self.media_input_kind_label(kind)
                ),
            },
            TelegramMediaResolutionError::MissingTelegramFilePath { file_id } => {
                match self.locale {
                    Locale::En => {
                        format!("Telegram did not return a downloadable path for file `{file_id}`")
                    }
                    Locale::Zh => format!("Telegram 没有返回文件 `{file_id}` 的可下载路径"),
                }
            }
            TelegramMediaResolutionError::UnsupportedNativeMedia {
                kind,
                file_name,
                mime_type,
            } => match self.locale {
                Locale::En => format!(
                    "this model/provider cannot process {} attachments in native media mode ({}; {}) and cannot submit them as regular files",
                    self.media_fallback_kind(*kind),
                    file_name,
                    mime_type
                ),
                Locale::Zh => format!(
                    "当前模型/接口暂不支持以原生媒体方式处理{}（{}；{}），且不能作为普通文件提交",
                    self.media_fallback_kind(*kind),
                    file_name,
                    mime_type
                ),
            },
            TelegramMediaResolutionError::Api { file_id, source } => match self.locale {
                Locale::En => format!("Telegram media API failed for file `{file_id}`: {source}"),
                Locale::Zh => format!("Telegram 媒体 API 处理文件 `{file_id}` 失败：{source}"),
            },
            TelegramMediaResolutionError::Io { path, source } => match self.locale {
                Locale::En => format!(
                    "could not prepare local download path `{}`: {source}",
                    path.display()
                ),
                Locale::Zh => {
                    format!("无法准备本地下载路径 `{}`：{source}", path.display())
                }
            },
            TelegramMediaResolutionError::InvalidFileUri { path } => match self.locale {
                Locale::En => format!(
                    "local download path cannot be represented as a file URI: `{}`",
                    path.display()
                ),
                Locale::Zh => format!("本地下载路径无法表示为 file URI：`{}`", path.display()),
            },
        }
    }

    fn media_file_size_label(self, actual: Option<u64>) -> String {
        match (self.locale, actual) {
            (Locale::En, Some(actual)) => format!("{actual} bytes"),
            (Locale::Zh, Some(actual)) => format!("{actual} 字节"),
            (Locale::En, None) => "unknown".into(),
            (Locale::Zh, None) => "未知".into(),
        }
    }

    fn media_input_kind_label(self, kind: &str) -> &'static str {
        match (self.locale, kind) {
            (Locale::En, "photo") => "photo",
            (Locale::En, "document") => "document",
            (Locale::En, "audio") => "audio",
            (Locale::En, "voice") => "voice",
            (Locale::En, "video") => "video",
            (Locale::Zh, "photo") => "图片",
            (Locale::Zh, "document") => "文件",
            (Locale::Zh, "audio") => "音频",
            (Locale::Zh, "voice") => "语音",
            (Locale::Zh, "video") => "视频",
            (Locale::En, _) => "media",
            (Locale::Zh, _) => "媒体",
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
    use crate::media::{
        TelegramMediaFallbackKind, TelegramMediaFallbackNotice, TelegramMediaResolutionError,
    };
    use noloong_agent::Locale;
    use noloong_agent_core::ToolPermissionOutcome;
    use std::path::PathBuf;

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

    #[test]
    fn ui_catalog_localizes_media_resolution_errors() {
        let catalog = TelegramUiCatalog::new(Locale::Zh);

        assert_eq!(
            catalog.media_resolution_failed(&TelegramMediaResolutionError::FileTooLarge {
                file_id: "file-1".into(),
                limit: 1024,
                actual: Some(2048),
            }),
            "媒体输入失败：文件 `file-1` 过大（限制：1024 字节，实际：2048 字节）"
        );
        assert_eq!(
            catalog.media_resolution_failed(&TelegramMediaResolutionError::MissingMime {
                file_id: "file-2".into(),
                kind: "document",
            }),
            "媒体输入失败：文件 `file-2` 缺少或无法识别 文件 的 MIME 类型"
        );
        assert_eq!(
            catalog.media_resolution_failed(&TelegramMediaResolutionError::Io {
                path: PathBuf::from("/tmp/noloong-telegram"),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            }),
            "媒体输入失败：无法准备本地下载路径 `/tmp/noloong-telegram`：denied"
        );
        assert_eq!(
            catalog.media_resolution_failed(
                &TelegramMediaResolutionError::UnsupportedNativeMedia {
                    kind: TelegramMediaFallbackKind::Audio,
                    file_name: "smoke.ogg".into(),
                    mime_type: "audio/ogg".into(),
                }
            ),
            "媒体输入失败：当前模型/接口暂不支持以原生媒体方式处理音频（smoke.ogg；audio/ogg），且不能作为普通文件提交"
        );
    }

    #[test]
    fn ui_catalog_localizes_media_fallback_notice() {
        let catalog = TelegramUiCatalog::new(Locale::Zh);

        assert_eq!(
            catalog.unsupported_media_fallback_notice(&TelegramMediaFallbackNotice {
                original_kind: TelegramMediaFallbackKind::Audio,
                file_name: "smoke.ogg".into(),
                mime_type: "audio/ogg".into(),
            }),
            "当前模型/接口暂不支持以原生媒体方式处理音频（smoke.ogg；audio/ogg），已按普通文件提交。"
        );
    }
}
