use crate::{HostEnvironment, Locale, PathStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKey {
    HostEnvironmentContext,
    HostExecStartDescription,
    HostExecReadDescription,
    HostExecWaitDescription,
    HostExecWriteDescription,
    HostExecTerminateDescription,
    HostExecListDescription,
    ManifestPatchDescription,
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
            Self::ManifestPatchDescription,
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

    pub fn assert_complete(locale: Locale) {
        let catalog = Self::new(locale);
        for key in MessageKey::all() {
            assert!(!catalog.message(*key).trim().is_empty());
        }
    }
}

fn en_message(key: MessageKey) -> &'static str {
    match key {
        MessageKey::HostEnvironmentContext => "Current host execution environment:",
        MessageKey::HostExecStartDescription => {
            "Start a host shell command in the background and return a job handle immediately."
        }
        MessageKey::HostExecReadDescription => {
            "Read buffered output from a background host command by cursor."
        }
        MessageKey::HostExecWaitDescription => {
            "Wait for a background host command to finish without killing it on timeout."
        }
        MessageKey::HostExecWriteDescription => {
            "Write text to a background host command stdin when stdin is enabled."
        }
        MessageKey::HostExecTerminateDescription => {
            "Terminate a background host command and return its latest status."
        }
        MessageKey::HostExecListDescription => "List background host command jobs in this session.",
        MessageKey::ManifestPatchDescription => {
            "Propose a manifest patch for the next product turn; it does not apply until approved."
        }
        MessageKey::ApprovalPrompt => "Review whether this tool call should be allowed.",
    }
}

fn zh_message(key: MessageKey) -> &'static str {
    match key {
        MessageKey::HostEnvironmentContext => "当前宿主机执行环境：",
        MessageKey::HostExecStartDescription => {
            "在宿主机后台启动 shell 命令，并立即返回 job handle。"
        }
        MessageKey::HostExecReadDescription => "按 cursor 读取后台宿主机命令的缓冲输出。",
        MessageKey::HostExecWaitDescription => "等待后台宿主机命令结束；超时时不会杀死该命令。",
        MessageKey::HostExecWriteDescription => "向已启用 stdin 的后台宿主机命令写入文本。",
        MessageKey::HostExecTerminateDescription => "终止后台宿主机命令，并返回其最新状态。",
        MessageKey::HostExecListDescription => "列出当前 session 中的后台宿主机命令 job。",
        MessageKey::ManifestPatchDescription => {
            "为下一个 product turn 提交 manifest patch 提案；审批前不会生效。"
        }
        MessageKey::ApprovalPrompt => "判断这个工具调用是否应该被允许。",
    }
}
