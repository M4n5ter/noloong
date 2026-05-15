use crate::{
    HostEnvironment, HostProcessCompletion, JobStatus, Locale, ManifestPatch, PathStyle,
    ProcessError, ProcessOutputStream,
    manifest::ManifestError,
    process::{
        PROCESS_EMPTY_COMMAND_MESSAGE, PROCESS_STDIN_DISABLED_PREFIX, PROCESS_STDIN_DISABLED_SUFFIX,
    },
};
use noloong_agent_core::ToolCall;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKey {
    HostEnvironmentContext,
    HostExecStartDescription,
    HostExecReadDescription,
    HostExecWaitDescription,
    HostExecWriteDescription,
    HostExecTerminateDescription,
    HostExecListDescription,
    SubagentSpawnDescription,
    SubagentWaitDescription,
    SubagentResultDescription,
    SubagentListDescription,
    FileWriteDescription,
    FileApplyPatchDescription,
    ManifestPatchDescription,
    HostCommandPermissionDescription,
    SubagentPermissionDescription,
    FileEditPermissionDescription,
    ManifestPatchPermissionDescription,
    ApprovalPrompt,
}

impl MessageKey {
    pub fn all() -> &'static [Self] {
        &[
            Self::HostEnvironmentContext,
            Self::HostExecStartDescription,
            Self::HostExecReadDescription,
            Self::HostExecWaitDescription,
            Self::HostExecWriteDescription,
            Self::HostExecTerminateDescription,
            Self::HostExecListDescription,
            Self::SubagentSpawnDescription,
            Self::SubagentWaitDescription,
            Self::SubagentResultDescription,
            Self::SubagentListDescription,
            Self::FileWriteDescription,
            Self::FileApplyPatchDescription,
            Self::ManifestPatchDescription,
            Self::HostCommandPermissionDescription,
            Self::SubagentPermissionDescription,
            Self::FileEditPermissionDescription,
            Self::ManifestPatchPermissionDescription,
            Self::ApprovalPrompt,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Catalog {
    locale: Locale,
}

impl Catalog {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn message(&self, key: MessageKey) -> &'static str {
        match self.locale {
            Locale::En => en_message(key),
            Locale::Zh => zh_message(key),
        }
    }

    pub fn render_host_environment(&self, environment: &HostEnvironment) -> String {
        let path_style = match environment.path_style {
            PathStyle::Unix => "unix",
            PathStyle::Windows => "windows",
        };
        match self.locale {
            Locale::En => format!(
                "{}\nOS: {}\nArchitecture: {}\nCurrent directory: {}\nDefault shell: {}\nAvailable shell hints: {}\nPath style: {}\nLocale: {}",
                self.message(MessageKey::HostEnvironmentContext),
                environment.os,
                environment.arch,
                environment.cwd.display(),
                environment.default_shell,
                environment.available_shell_hints.join(", "),
                path_style,
                environment.locale.code()
            ),
            Locale::Zh => format!(
                "{}\n操作系统: {}\n架构: {}\n当前目录: {}\n默认 shell: {}\n可用 shell 提示: {}\n路径风格: {}\n语言: {}",
                self.message(MessageKey::HostEnvironmentContext),
                environment.os,
                environment.arch,
                environment.cwd.display(),
                environment.default_shell,
                environment.available_shell_hints.join(", "),
                path_style,
                environment.locale.code()
            ),
        }
    }

    pub fn render_approval_prompt(&self, tool_call: &ToolCall) -> String {
        match self.locale {
            Locale::En => format!(
                "{} Tool: `{}`. Arguments: {}",
                self.message(MessageKey::ApprovalPrompt),
                tool_call.name,
                tool_call.arguments
            ),
            Locale::Zh => format!(
                "{} 工具：`{}`。参数：{}",
                self.message(MessageKey::ApprovalPrompt),
                tool_call.name,
                tool_call.arguments
            ),
        }
    }

    pub fn approval_allow_reason(&self) -> &'static str {
        match self.locale {
            Locale::En => "allowed by application approval policy",
            Locale::Zh => "应用审批策略已允许该工具调用",
        }
    }

    pub fn approval_human_required_reason(&self) -> &'static str {
        match self.locale {
            Locale::En => "human approval required",
            Locale::Zh => "需要人工审批",
        }
    }

    pub fn approval_auto_review_human_fallback_reason(&self) -> &'static str {
        match self.locale {
            Locale::En => "auto-review is disabled; human approval required",
            Locale::Zh => "自动审查未启用；需要人工审批",
        }
    }

    pub fn approval_auto_review_denied_reason(&self) -> &'static str {
        match self.locale {
            Locale::En => "auto-review is disabled and human fallback is disabled",
            Locale::Zh => "自动审查未启用，且未启用人工审批回退",
        }
    }

    pub fn render_background_completion(
        &self,
        completion: &HostProcessCompletion,
        output_preview: &str,
    ) -> String {
        let job_id = &completion.snapshot.job_id;
        match self.locale {
            Locale::En => format!(
                "Background host command completed.\n\
                 Job ID: {job_id}\n\
                 Status: {}\n\
                 Command: {}\n\
                 Shell: {}\n\
                 CWD: {}\n\
                 Started at ms: {}\n\
                 Ended at ms: {}\n\
                 Output cursor: {}\n\
                 Dropped before seq: {}\n\
                 Output preview truncated: {}\n\n\
                 Output preview:\n{}\n\n\
                 Use `host.exec.read` with `jobId` `{job_id}` and `afterSeq` to inspect more output.",
                self.render_job_status(&completion.snapshot.status),
                completion.snapshot.command,
                completion.snapshot.shell,
                completion.snapshot.cwd.display(),
                completion.snapshot.started_at_ms,
                completion
                    .snapshot
                    .ended_at_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| self.unknown_value().into()),
                completion.output.next_cursor,
                completion.output.dropped_before_seq,
                completion.output.truncated,
                output_preview,
            ),
            Locale::Zh => format!(
                "后台宿主机命令已完成。\n\
                 Job ID：{job_id}\n\
                 状态：{}\n\
                 命令：{}\n\
                 Shell：{}\n\
                 工作目录：{}\n\
                 启动时间 ms：{}\n\
                 结束时间 ms：{}\n\
                 输出 cursor：{}\n\
                 已丢弃 seq 之前输出：{}\n\
                 输出预览是否已截断：{}\n\n\
                 输出预览：\n{}\n\n\
                 如需查看更多输出，请调用 `host.exec.read`，使用 `jobId` `{job_id}` 和 `afterSeq` cursor。",
                self.render_job_status(&completion.snapshot.status),
                completion.snapshot.command,
                completion.snapshot.shell,
                completion.snapshot.cwd.display(),
                completion.snapshot.started_at_ms,
                completion
                    .snapshot
                    .ended_at_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| self.unknown_value().into()),
                completion.output.next_cursor,
                completion.output.dropped_before_seq,
                completion.output.truncated,
                output_preview,
            ),
        }
    }

    pub fn no_buffered_output(&self) -> &'static str {
        match self.locale {
            Locale::En => "(no buffered output)",
            Locale::Zh => "（没有缓冲输出）",
        }
    }

    pub fn unknown_value(&self) -> &'static str {
        match self.locale {
            Locale::En => "unknown",
            Locale::Zh => "未知",
        }
    }

    pub fn render_process_stream(&self, stream: ProcessOutputStream) -> &'static str {
        match stream {
            ProcessOutputStream::Stdout => "stdout",
            ProcessOutputStream::Stderr => "stderr",
        }
    }

    pub fn render_job_status(&self, status: &JobStatus) -> String {
        match (self.locale, status) {
            (Locale::En, JobStatus::Running) => "running".into(),
            (Locale::Zh, JobStatus::Running) => "运行中".into(),
            (Locale::En, JobStatus::Exited { code }) => {
                format!("exited(code={})", optional_i32(*code, self.unknown_value()))
            }
            (Locale::Zh, JobStatus::Exited { code }) => {
                format!(
                    "已退出（退出码={}）",
                    optional_i32(*code, self.unknown_value())
                )
            }
            (Locale::En, JobStatus::Terminated) => "terminated".into(),
            (Locale::Zh, JobStatus::Terminated) => "已终止".into(),
            (Locale::En, JobStatus::Failed { error }) => format!("failed({error})"),
            (Locale::Zh, JobStatus::Failed { error }) => format!("失败（{error}）"),
        }
    }

    pub fn render_tool_output_overflow(&self, params: ToolOutputOverflowRender<'_>) -> String {
        let path = params.path.display();
        match self.locale {
            Locale::En => format!(
                "Tool output was too large to inline and has been written to a temporary JSON file.\n\
                 Path: {path}\n\
                 Tool: {}\n\
                 Tool call ID: {}\n\
                 Original bytes: {}\n\
                 Inline limit bytes: {}\n\
                 Omitted preview bytes: {}\n\n\
                 Output preview head:\n{}\n\n\
                 Output preview tail:\n{}\n\n\
                 Use host command tooling to read the file when the full result is needed.",
                params.tool_name,
                params.tool_call_id,
                params.original_bytes,
                params.inline_limit_bytes,
                params.preview_omitted_bytes,
                params.preview_head,
                params.preview_tail,
            ),
            Locale::Zh => format!(
                "工具输出过长，无法完整内联；完整 `ToolOutput` 已写入临时 JSON 文件。\n\
                 路径：{path}\n\
                 工具：{}\n\
                 工具调用 ID：{}\n\
                 原始字节数：{}\n\
                 内联上限字节数：{}\n\
                 预览省略字节数：{}\n\n\
                 输出开头预览：\n{}\n\n\
                 输出结尾预览：\n{}\n\n\
                 如需完整结果，请使用宿主机命令工具读取该文件。",
                params.tool_name,
                params.tool_call_id,
                params.original_bytes,
                params.inline_limit_bytes,
                params.preview_omitted_bytes,
                params.preview_head,
                params.preview_tail,
            ),
        }
    }

    pub fn render_tool_output_overflow_failure(
        &self,
        params: ToolOutputOverflowFailureRender<'_>,
    ) -> String {
        match self.locale {
            Locale::En => format!(
                "Tool output exceeded the inline limit, but the full output could not be written to a temporary file.\n\
                 Tool: {}\n\
                 Tool call ID: {}\n\
                 Inline limit bytes: {}\n\
                 Error: {}",
                params.tool_name, params.tool_call_id, params.inline_limit_bytes, params.error,
            ),
            Locale::Zh => format!(
                "工具输出超过内联上限，但完整输出无法写入临时文件。\n\
                 工具：{}\n\
                 工具调用 ID：{}\n\
                 内联上限字节数：{}\n\
                 错误：{}",
                params.tool_name, params.tool_call_id, params.inline_limit_bytes, params.error,
            ),
        }
    }

    pub fn failed_to_serialize_tool_output(&self, error: impl std::fmt::Display) -> String {
        match self.locale {
            Locale::En => format!("failed to serialize tool output: {error}"),
            Locale::Zh => format!("序列化工具输出失败：{error}"),
        }
    }

    pub fn failed_to_persist_tool_output(&self, error: impl std::fmt::Display) -> String {
        match self.locale {
            Locale::En => format!("failed to persist oversized tool output: {error}"),
            Locale::Zh => format!("持久化超大工具输出失败：{error}"),
        }
    }

    pub fn render_process_error(&self, error: &ProcessError) -> String {
        match error {
            ProcessError::Invalid(message) => self.render_invalid_process_error(message),
            ProcessError::UnknownJob(job_id) => self.render_unknown_process_job(job_id),
            ProcessError::Spawn(message) => self.render_process_spawn_error(message),
            ProcessError::Io(message) => self.render_process_io_error(message),
        }
    }

    fn render_invalid_process_error(&self, message: &str) -> String {
        if message == PROCESS_EMPTY_COMMAND_MESSAGE {
            return self.command_must_not_be_empty().into();
        }
        if let Some(job_id) = stdin_disabled_job_id(message) {
            return self.render_job_does_not_accept_stdin(job_id);
        }
        match self.locale {
            Locale::En => format!("invalid process request: {message}"),
            Locale::Zh => format!("无效的进程请求：{message}"),
        }
    }

    fn render_unknown_process_job(&self, job_id: &str) -> String {
        match self.locale {
            Locale::En => format!("unknown process job: {job_id}"),
            Locale::Zh => format!("未知进程 job：{job_id}"),
        }
    }

    fn render_process_spawn_error(&self, message: &str) -> String {
        match self.locale {
            Locale::En => format!("failed to spawn process: {message}"),
            Locale::Zh => format!("启动进程失败：{message}"),
        }
    }

    fn render_process_io_error(&self, message: &str) -> String {
        match self.locale {
            Locale::En => format!("process io failed: {message}"),
            Locale::Zh => format!("进程 IO 失败：{message}"),
        }
    }

    pub fn command_must_not_be_empty(&self) -> &'static str {
        match self.locale {
            Locale::En => PROCESS_EMPTY_COMMAND_MESSAGE,
            Locale::Zh => "命令不能为空",
        }
    }

    pub fn render_job_does_not_accept_stdin(&self, job_id: &str) -> String {
        match self.locale {
            Locale::En => {
                format!("{PROCESS_STDIN_DISABLED_PREFIX}{job_id}{PROCESS_STDIN_DISABLED_SUFFIX}")
            }
            Locale::Zh => format!("job {job_id} 未启用 stdin"),
        }
    }

    pub fn render_manifest_patch_summary(&self, patch: &ManifestPatch) -> String {
        match (self.locale, patch) {
            (Locale::En, _) => patch.summary(),
            (Locale::Zh, ManifestPatch::ReplaceSystemPrompt { .. }) => "替换系统提示词".into(),
            (Locale::Zh, ManifestPatch::UseBuiltInSystemPrompt) => "使用内置系统提示词".into(),
            (Locale::Zh, ManifestPatch::SetBuiltInSystemPromptProfile { profile }) => {
                format!("设置内置系统提示词 profile 为 {}", profile.as_str())
            }
            (Locale::Zh, ManifestPatch::UpsertSystemPromptAddition { addition }) => {
                format!("新增或更新系统提示词追加项 {}", addition.id)
            }
            (Locale::Zh, ManifestPatch::RemoveSystemPromptAddition { id }) => {
                format!("移除系统提示词追加项 {id}")
            }
            (Locale::Zh, ManifestPatch::SetSystemPromptAdditionEnabled { id, enabled }) => {
                format!("设置系统提示词追加项 {id} enabled={enabled}")
            }
            (Locale::Zh, ManifestPatch::ReorderSystemPromptAdditions { .. }) => {
                "重排系统提示词追加项".into()
            }
            (Locale::Zh, ManifestPatch::ClearSystemPromptAdditions) => {
                "清空系统提示词追加项".into()
            }
            (Locale::Zh, ManifestPatch::SetLocale { locale }) => {
                format!("设置语言为 {}", locale.code())
            }
            (Locale::Zh, ManifestPatch::EnableTool { tool_name }) => {
                format!("启用工具 {}", tool_name.as_str())
            }
            (Locale::Zh, ManifestPatch::DisableTool { tool_name }) => {
                format!("禁用工具 {}", tool_name.as_str())
            }
            (Locale::Zh, ManifestPatch::UpdateApprovalPolicy { .. }) => "更新审批策略".into(),
            (Locale::Zh, ManifestPatch::UpdateFileEditToolPolicy { policy }) => {
                format!("更新文件编辑工具策略为 {}", policy.as_str())
            }
            (Locale::Zh, ManifestPatch::RegisterPlugin { plugin }) => {
                format!("注册插件 {}", plugin.summary())
            }
            (Locale::Zh, ManifestPatch::SetPluginEnabled { plugin_id, enabled }) => {
                format!("设置插件 {plugin_id} enabled={enabled}")
            }
            (Locale::Zh, ManifestPatch::RemovePlugin { plugin_id }) => {
                format!("移除插件 {plugin_id}")
            }
            (Locale::Zh, ManifestPatch::ReservedPhaseProfile { description, .. }) => {
                format!("保留的阶段配置补丁：{description}")
            }
        }
    }

    pub fn render_manifest_error(&self, error: &ManifestError) -> String {
        match (self.locale, error) {
            (Locale::En, _) => error.to_string(),
            (Locale::Zh, ManifestError::Invalid(message)) => {
                format!("无效的 manifest 补丁：{message}")
            }
            (Locale::Zh, ManifestError::UnknownTool(tool_name)) => {
                format!("未知内置工具：{tool_name}")
            }
            (Locale::Zh, ManifestError::UnknownSystemPromptAddition(id)) => {
                format!("未知系统提示词追加项：{id}")
            }
            (Locale::Zh, ManifestError::UnknownPlugin(plugin_id)) => {
                format!("未知插件：{plugin_id}")
            }
            (Locale::Zh, ManifestError::PluginAlreadyExists(plugin_id)) => {
                format!("插件已存在：{plugin_id}")
            }
            (Locale::Zh, ManifestError::UnknownProposal(proposal_id)) => {
                format!("未知 manifest 提案：{proposal_id}")
            }
            (Locale::Zh, ManifestError::Unsupported(message)) => {
                format!("不支持的 manifest 补丁：{message}")
            }
        }
    }

    pub fn render_tool_input_error(&self, error: impl std::fmt::Display) -> String {
        match self.locale {
            Locale::En => format!("invalid tool input: {error}"),
            Locale::Zh => format!("无效的工具输入：{error}"),
        }
    }

    pub fn missing_manifest_patch_argument(&self) -> &'static str {
        match self.locale {
            Locale::En => "missing patch argument",
            Locale::Zh => "缺少 patch 参数",
        }
    }

    pub fn assert_complete(locale: Locale) {
        let catalog = Self::new(locale);
        for key in MessageKey::all() {
            assert!(!catalog.message(*key).trim().is_empty());
        }
    }
}

pub struct ToolOutputOverflowRender<'a> {
    pub path: &'a Path,
    pub tool_name: &'a str,
    pub tool_call_id: &'a str,
    pub original_bytes: usize,
    pub inline_limit_bytes: usize,
    pub preview_head: &'a str,
    pub preview_tail: &'a str,
    pub preview_omitted_bytes: usize,
}

pub struct ToolOutputOverflowFailureRender<'a> {
    pub tool_name: &'a str,
    pub tool_call_id: &'a str,
    pub inline_limit_bytes: usize,
    pub error: &'a str,
}

fn optional_i32(value: Option<i32>, unknown: &str) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| unknown.into())
}

fn stdin_disabled_job_id(message: &str) -> Option<&str> {
    message
        .strip_prefix(PROCESS_STDIN_DISABLED_PREFIX)?
        .strip_suffix(PROCESS_STDIN_DISABLED_SUFFIX)
}

fn en_message(key: MessageKey) -> &'static str {
    match key {
        MessageKey::HostEnvironmentContext => "Current host execution environment:",
        MessageKey::HostExecStartDescription => {
            "Start a host shell command as a background job. Set foregroundWaitMs to return quick results inline; otherwise use the returned jobId with host.exec.read, host.exec.wait, host.exec.write, or host.exec.terminate."
        }
        MessageKey::HostExecReadDescription => {
            "Read buffered stdout/stderr from a background host command without consuming it. Use jobId with afterSeq as the cursor and maxBytes to bound the returned output."
        }
        MessageKey::HostExecWaitDescription => {
            "Wait for a background host command to finish and return its current outcome. A timeout only returns the latest state; it does not kill the job."
        }
        MessageKey::HostExecWriteDescription => {
            "Write text to a background host command stdin. This only works for jobs started with pipeStdin enabled."
        }
        MessageKey::HostExecTerminateDescription => {
            "Request termination of a background host command by jobId and return the latest job status."
        }
        MessageKey::HostExecListDescription => {
            "List background host command jobs for the current session with their latest status."
        }
        MessageKey::SubagentSpawnDescription => {
            "Spawn a direct child subagent for a bounded task and start it with the provided prompt. Always use agent.subagent.wait or agent.subagent.result to collect real status and final output; never invent sessionId, status, or finalText in prose."
        }
        MessageKey::SubagentWaitDescription => {
            "Wait for one or more direct child subagents to settle and return each real status plus final assistant output when available. Timeout does not abort subagents; do not simulate wait results."
        }
        MessageKey::SubagentResultDescription => {
            "Read the current real status and final assistant output for one direct child subagent without waiting."
        }
        MessageKey::SubagentListDescription => {
            "List direct child subagents for the current session with lightweight status information."
        }
        MessageKey::FileWriteDescription => {
            "Edit a text file on the host filesystem. Provide content for a complete file write, or provide oldString and newString for an exact replacement; replaceAll controls whether every match is replaced."
        }
        MessageKey::FileApplyPatchDescription => {
            "Apply a strict V4A patch format on the host filesystem. The patch can add, update, delete, or move files and must use the required Begin/End Patch markers. Minimal example:\n\
*** Begin Patch\n\
*** Add File: notes.txt\n\
+hello\n\
*** Update File: src/lib.rs\n\
@@\n\
-old\n\
+new\n\
*** Delete File: old.txt\n\
*** Move File: draft.txt -> final.txt\n\
*** End Patch"
        }
        MessageKey::ManifestPatchDescription => {
            "Propose a manifest patch that may change future agent session behavior. The proposal is recorded for a future turn and does not apply until approved."
        }
        MessageKey::HostCommandPermissionDescription => {
            "Start and control host processes, including reading output, writing stdin, waiting, listing, or terminating jobs."
        }
        MessageKey::SubagentPermissionDescription => {
            "Spawn and inspect direct child subagents for the current session."
        }
        MessageKey::FileEditPermissionDescription => {
            "Modify the host filesystem through file editing tools, including writing, replacing, moving, or deleting paths."
        }
        MessageKey::ManifestPatchPermissionDescription => {
            "Propose agent manifest changes that can alter future session behavior after approval."
        }
        MessageKey::ApprovalPrompt => "Review whether this tool call should be allowed.",
    }
}

fn zh_message(key: MessageKey) -> &'static str {
    match key {
        MessageKey::HostEnvironmentContext => "当前宿主机执行环境：",
        MessageKey::HostExecStartDescription => {
            "将宿主机 shell 命令作为后台 job 启动。设置 foregroundWaitMs 可让快速结果直接内联返回；否则请使用返回的 jobId 继续调用 host.exec.read、host.exec.wait、host.exec.write 或 host.exec.terminate。"
        }
        MessageKey::HostExecReadDescription => {
            "非破坏性读取后台宿主机命令的 stdout/stderr 缓冲输出。使用 jobId 和 afterSeq 作为 cursor，并用 maxBytes 限制返回输出大小。"
        }
        MessageKey::HostExecWaitDescription => {
            "等待后台宿主机命令结束并返回当前结果。超时只会返回最新状态，不会杀死该 job。"
        }
        MessageKey::HostExecWriteDescription => {
            "向后台宿主机命令的 stdin 写入文本。只有启动时启用了 pipeStdin 的 job 才支持该操作。"
        }
        MessageKey::HostExecTerminateDescription => {
            "通过 jobId 请求终止后台宿主机命令，并返回该 job 的最新状态。"
        }
        MessageKey::HostExecListDescription => {
            "列出当前 session 中的后台宿主机命令 job 及其最新状态。"
        }
        MessageKey::SubagentSpawnDescription => {
            "为有边界的任务创建直接子 agent，并用给定 prompt 启动它。必须使用 agent.subagent.wait 或 agent.subagent.result 收集真实状态和最终 assistant 输出；不要在文字里编造 sessionId、status 或 finalText。"
        }
        MessageKey::SubagentWaitDescription => {
            "等待一个或多个直接子 agent 进入终态，并返回每个子 agent 的真实状态以及可用的最终 assistant 输出。超时不会中止子 agent；不要模拟等待结果。"
        }
        MessageKey::SubagentResultDescription => {
            "读取一个直接子 agent 的真实当前状态和最终 assistant 输出，不等待。"
        }
        MessageKey::SubagentListDescription => "列出当前 session 的直接子 agent 及轻量状态信息。",
        MessageKey::FileWriteDescription => {
            "编辑宿主机文件系统中的文本文件。提供 content 表示完整写入文件；或提供 oldString 和 newString 表示精确替换，replaceAll 控制是否替换所有匹配项。"
        }
        MessageKey::FileApplyPatchDescription => {
            "在宿主机文件系统中应用严格 V4A patch 格式。patch 可以新增、更新、删除或移动文件，并且必须使用 Begin/End Patch 标记。最小示例：\n\
*** Begin Patch\n\
*** Add File: notes.txt\n\
+hello\n\
*** Update File: src/lib.rs\n\
@@\n\
-old\n\
+new\n\
*** Delete File: old.txt\n\
*** Move File: draft.txt -> final.txt\n\
*** End Patch"
        }
        MessageKey::ManifestPatchDescription => {
            "提交可能改变未来 agent session 行为的 manifest patch 提案。该提案会记录到后续轮次，审批通过前不会生效。"
        }
        MessageKey::HostCommandPermissionDescription => {
            "启动和控制宿主机进程，包括读取输出、写入 stdin、等待、列出或终止 job。"
        }
        MessageKey::SubagentPermissionDescription => "为当前 session 创建并查看直接子 agent。",
        MessageKey::FileEditPermissionDescription => {
            "通过文件编辑工具修改宿主机文件系统，包括写入、替换、移动或删除路径。"
        }
        MessageKey::ManifestPatchPermissionDescription => {
            "提交 agent manifest 变更提案；审批后可改变未来 session 行为。"
        }
        MessageKey::ApprovalPrompt => "判断这个工具调用是否应该被允许。",
    }
}
