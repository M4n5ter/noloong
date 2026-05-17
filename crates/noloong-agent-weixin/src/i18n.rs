use noloong_agent::Locale;

#[derive(Clone, Copy, Debug)]
pub struct WeixinCatalog {
    locale: Locale,
}

impl WeixinCatalog {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn help(&self) -> &'static str {
        match self.locale {
            Locale::Zh => {
                "微信控制命令必须以 / 或 ／ 开头；没有前缀的中文会作为普通消息发给 agent。\n\n\
常用\n\n\
/状态：查看当前会话。\n\n\
/新会话：创建新会话。\n\n\
/会话：列出会话。\n\n\
/切换 1、/删除 1：管理会话。\n\n\
运行\n\n\
/运行配置：查看 profile、工具、插件和消息数。\n\n\
/队列：查看队列。\n\n\
/队列 <文本>：加入 follow-up。\n\n\
/清空队列：清空队列。\n\n\
/进程：列出后台进程。\n\n\
/进程 1、/进程 1 等待、/进程 1 终止：管理后台进程。\n\n\
协作\n\n\
/审批：查看待审批工具。\n\n\
/同意 1、/拒绝 1：处理审批。\n\n\
/子任务 <prompt>：创建子任务。"
            }
            _ => {
                "Weixin control commands must start with / or ／. Text without the prefix is normal agent input.\n\n\
Common\n\n\
/status: Show the current session.\n\n\
/new: Create a new session.\n\n\
/sessions: List sessions.\n\n\
/switch 1, /delete 1: Manage sessions.\n\n\
Runtime\n\n\
/config: Show profile, tools, plugins, and message count.\n\n\
/queue: Show queues.\n\n\
/queue <text>: Add a follow-up.\n\n\
/clear_queue: Clear queues.\n\n\
/processes: List background processes.\n\n\
/process 1, /process 1 wait, /process 1 terminate: Manage a background process.\n\n\
Collaboration\n\n\
/approvals: List pending tool approvals.\n\n\
/approve 1, /deny 1: Resolve one approval.\n\n\
/subagent <prompt>: Spawn a subagent."
            }
        }
    }

    pub fn no_current_session(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "当前没有会话。发送普通消息会自动创建会话。",
            _ => "There is no current session. Send a normal message to create one.",
        }
    }

    pub fn no_session(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "当前没有会话。",
            _ => "There is no current session.",
        }
    }

    pub fn no_approvals(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "没有待处理审批。",
            _ => "There are no pending approvals.",
        }
    }

    pub fn no_matching_approval(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "没有匹配的待处理审批。",
            _ => "No matching pending approval.",
        }
    }

    pub fn stale_session_selector(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "会话编号已过期或不匹配。请使用下面这份当前列表重新选择。",
            _ => "The session selector is stale or no longer matches. Use the current list below.",
        }
    }

    pub fn stale_process_selector(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "进程编号已过期或不匹配。请使用下面这份当前列表重新选择。",
            _ => "The process selector is stale or no longer matches. Use the current list below.",
        }
    }

    pub fn stale_approval_selector(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "审批编号已过期或不匹配。请使用下面这份当前列表重新选择。",
            _ => "The approval selector is stale or no longer matches. Use the current list below.",
        }
    }

    pub fn process_usage(&self) -> &'static str {
        match self.locale {
            Locale::Zh => {
                "用法\n\n/进程\n\n/进程 1\n\n/进程 <job-id>\n\n/进程 <job-id> 等待\n\n/进程 <job-id> 终止"
            }
            _ => {
                "Usage\n\n/processes\n\n/process 1\n\n/process <job-id>\n\n/process <job-id> wait\n\n/process <job-id> terminate"
            }
        }
    }

    pub fn subagent_usage(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "用法：/子任务 <prompt>",
            _ => "Usage: /subagent <prompt>",
        }
    }

    pub fn subagent_created(&self, session_id: &str, status: &str) -> String {
        match self.locale {
            Locale::Zh => format!("子任务已创建\n\n会话：{session_id}\n\n状态：{status}"),
            _ => format!("Subagent created\n\nSession: {session_id}\n\nStatus: {status}"),
        }
    }

    pub fn still_running(&self) -> &'static str {
        match self.locale {
            Locale::Zh => "任务仍在运行，我会在有结果后继续发送。",
            _ => "The task is still running. I will send the result when it is ready.",
        }
    }
}
