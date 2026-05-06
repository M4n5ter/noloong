use noloong_agent::Locale;
use noloong_agent_core::ToolPermissionOutcome;

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

    pub fn approval_permissions(self, permissions: &str) -> String {
        match self.locale {
            Locale::En => format!("Permissions: {permissions}"),
            Locale::Zh => format!("权限：{permissions}"),
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

    pub fn run_failed(self, error: &str) -> String {
        match self.locale {
            Locale::En => format!("Run failed: {error}"),
            Locale::Zh => format!("运行失败：{error}"),
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
}

impl Default for TelegramUiCatalog {
    fn default() -> Self {
        Self::new(Locale::En)
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramUiCatalog;
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
}
